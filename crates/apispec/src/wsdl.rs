//! WSDL 1.1 with SOAP 1.1 and 1.2 bindings: one endpoint per operation of
//! each port, with an envelope built from the schema in `<types>`.
//! Imported schemas and WSDLs are not fetched.

use crate::{ApiDefinition, ApiEndpoint, ApiParam, MAX_ENDPOINTS, ParamLocation, SpecFormat};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;

const WSDL_NS: &str = "http://schemas.xmlsoap.org/wsdl/";
const SOAP11_NS: &str = "http://schemas.xmlsoap.org/wsdl/soap/";
const SOAP12_NS: &str = "http://schemas.xmlsoap.org/wsdl/soap12/";
const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema";
/// Most elements read from one document.
const MAX_NODES: usize = 200_000;
/// Deepest nesting of generated envelope elements.
const MAX_ENVELOPE_DEPTH: usize = 4;

/// An element: namespace URI, local name, attributes (local name → value).
#[derive(Debug, Default)]
struct Node {
    ns: String,
    name: String,
    attrs: HashMap<String, String>,
    children: Vec<Node>,
}

impl Node {
    fn attr(&self, name: &str) -> &str {
        self.attrs.get(name).map(String::as_str).unwrap_or_default()
    }

    fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }

    fn kids<'a>(&'a self, ns: &'a str, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.is(ns, name))
    }

    fn descendants<'a>(&'a self, out: &mut Vec<&'a Node>, ns: &str, name: &str) {
        for c in &self.children {
            if c.is(ns, name) {
                out.push(c);
            }
            c.descendants(out, ns, name);
        }
    }
}

/// `tns:Name` → `Name`.
fn local(qname: &str) -> &str {
    qname.rsplit(':').next().unwrap_or(qname)
}

/// Reads the document into a tree with namespaces resolved.
fn read_tree(text: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(true);
    // Each open element: the node and the prefixes it declared.
    let mut stack: Vec<(Node, HashMap<String, String>)> = Vec::new();
    let mut scopes: Vec<HashMap<String, String>> = vec![HashMap::new()];
    let mut root: Option<Node> = None;
    let mut count = 0usize;
    let open = |e: &BytesStart<'_>,
                scopes: &mut Vec<HashMap<String, String>>|
     -> Result<(Node, HashMap<String, String>), String> {
        let mut declared = HashMap::new();
        let mut attrs = HashMap::new();
        for a in e.attributes().flatten() {
            let key = a.key.as_ref().to_string();
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .unwrap_or_else(|_| a.value.to_string());
            if key == "xmlns" {
                declared.insert(String::new(), value);
            } else if let Some(prefix) = key.strip_prefix("xmlns:") {
                declared.insert(prefix.to_string(), value);
            } else {
                attrs.insert(local(&key).to_string(), value);
            }
        }
        let mut scope = scopes.last().cloned().unwrap_or_default();
        scope.extend(declared.clone());
        let qname = e.name().as_ref().to_string();
        let (prefix, name) = match qname.split_once(':') {
            Some((p, n)) => (p.to_string(), n.to_string()),
            None => (String::new(), qname.clone()),
        };
        let ns = scope.get(&prefix).cloned().unwrap_or_default();
        scopes.push(scope);
        Ok((
            Node {
                ns,
                name,
                attrs,
                children: Vec::new(),
            },
            declared,
        ))
    };
    loop {
        let event = reader
            .read_event()
            .map_err(|e| format!("not well-formed XML: {e}"))?;
        match event {
            Event::Start(e) => {
                count += 1;
                if count > MAX_NODES || stack.len() > 200 {
                    return Err("the XML document is too large or too deep".into());
                }
                stack.push(open(&e, &mut scopes)?);
            }
            Event::Empty(e) => {
                count += 1;
                if count > MAX_NODES {
                    return Err("the XML document is too large".into());
                }
                let (node, _) = open(&e, &mut scopes)?;
                scopes.pop();
                match stack.last_mut() {
                    Some((parent, _)) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::End(_) => {
                scopes.pop();
                let Some((node, _)) = stack.pop() else {
                    return Err("not well-formed XML: unexpected end tag".into());
                };
                match stack.last_mut() {
                    Some((parent, _)) => parent.children.push(node),
                    None => root = Some(node),
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    root.ok_or_else(|| "the XML document is empty".into())
}

/// The schema elements and named complex types of `<types>`.
struct Schema<'a> {
    elements: HashMap<&'a str, &'a Node>,
    types: HashMap<&'a str, &'a Node>,
    qualified: bool,
}

impl<'a> Schema<'a> {
    fn new(defs: &'a Node) -> Schema<'a> {
        let mut schema = Schema {
            elements: HashMap::new(),
            types: HashMap::new(),
            qualified: false,
        };
        for types in defs.kids(WSDL_NS, "types") {
            for s in types.kids(XSD_NS, "schema") {
                schema.qualified |= s.attr("elementFormDefault") == "qualified";
                for c in &s.children {
                    if c.is(XSD_NS, "element") {
                        schema.elements.insert(c.attr("name"), c);
                    } else if c.is(XSD_NS, "complexType") {
                        schema.types.insert(c.attr("name"), c);
                    }
                }
            }
        }
        schema
    }

    /// The child elements of an element (inline or named complex type).
    fn children_of(&self, element: &'a Node) -> Vec<&'a Node> {
        let complex = element
            .kids(XSD_NS, "complexType")
            .next()
            .or_else(|| self.types.get(local(element.attr("type"))).copied());
        let mut out = Vec::new();
        if let Some(complex) = complex {
            for group in ["sequence", "all", "choice"] {
                let mut found = Vec::new();
                complex.descendants(&mut found, XSD_NS, group);
                for g in found {
                    out.extend(g.kids(XSD_NS, "element"));
                }
            }
        }
        out
    }

    /// Writes `<prefix:name>…</prefix:name>` with children or a value.
    fn write(
        &self,
        element: &'a Node,
        prefix: &str,
        depth: usize,
        out: &mut String,
        params: &mut Vec<ApiParam>,
    ) {
        let element = match element.attr("ref") {
            "" => element,
            r => match self.elements.get(local(r)) {
                Some(e) => e,
                None => element,
            },
        };
        let name = element.attr("name");
        if name.is_empty() {
            return;
        }
        let tag = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}:{name}")
        };
        let children = if depth < MAX_ENVELOPE_DEPTH {
            self.children_of(element)
        } else {
            Vec::new()
        };
        if children.is_empty() {
            let value = sample(local(element.attr("type")));
            params.push(ApiParam {
                name: name.to_string(),
                location: ParamLocation::Body,
                required: element.attr("minOccurs") != "0",
                example: value.clone(),
            });
            out.push_str(&format!("<{tag}>{value}</{tag}>"));
        } else {
            out.push_str(&format!("<{tag}>"));
            let child_prefix = if self.qualified { prefix } else { "" };
            for c in children {
                self.write(c, child_prefix, depth + 1, out, params);
            }
            out.push_str(&format!("</{tag}>"));
        }
    }
}

/// A sample value for an XML Schema type.
fn sample(xsd_type: &str) -> String {
    match xsd_type {
        "int" | "integer" | "long" | "short" | "byte" | "unsignedInt" | "unsignedLong"
        | "nonNegativeInteger" | "positiveInteger" => "1".into(),
        "decimal" | "double" | "float" => "1.5".into(),
        "boolean" => "true".into(),
        "date" => "2024-01-31".into(),
        "dateTime" => "2024-01-31T12:00:00Z".into(),
        _ => "?".into(),
    }
}

pub(crate) fn parse(text: &str, base: Option<&url::Url>) -> Result<ApiDefinition, String> {
    let defs = read_tree(text)?;
    if !defs.is(WSDL_NS, "definitions") {
        return Err("not a WSDL 1.1 document (WSDL 2.0 is not supported)".into());
    }
    let tns = defs.attr("targetNamespace").to_string();
    let schema = Schema::new(&defs);
    let mut warnings = Vec::new();
    if defs.kids(WSDL_NS, "import").next().is_some() {
        warnings.push("Imported WSDL documents are not fetched.".into());
    }
    let messages: HashMap<&str, &Node> = defs
        .kids(WSDL_NS, "message")
        .map(|m| (m.attr("name"), m))
        .collect();
    let port_types: HashMap<&str, &Node> = defs
        .kids(WSDL_NS, "portType")
        .map(|p| (p.attr("name"), p))
        .collect();
    let bindings: HashMap<&str, &Node> = defs
        .kids(WSDL_NS, "binding")
        .map(|b| (b.attr("name"), b))
        .collect();
    let mut endpoints = Vec::new();
    for service in defs.kids(WSDL_NS, "service") {
        for port in service.kids(WSDL_NS, "port") {
            let Some(binding) = bindings.get(local(port.attr("binding"))) else {
                warnings.push(format!(
                    "Port {} names an unknown binding.",
                    port.attr("name")
                ));
                continue;
            };
            let soap12 = binding.children.iter().any(|c| c.ns == SOAP12_NS);
            let soap_ns = if soap12 { SOAP12_NS } else { SOAP11_NS };
            if !binding
                .children
                .iter()
                .any(|c| c.ns == SOAP11_NS || c.ns == SOAP12_NS)
            {
                continue; // HTTP bindings are not SOAP.
            }
            let Some(address) = port.kids(soap_ns, "address").next() else {
                continue;
            };
            let location = address.attr("location");
            let url = match url::Url::parse(location) {
                Ok(u) if matches!(u.scheme(), "http" | "https") => u.to_string(),
                _ => match base.and_then(|b| b.join(location).ok()) {
                    Some(u) => u.to_string(),
                    None => {
                        warnings.push(format!(
                            "Port {} has no absolute address; give a base URL.",
                            port.attr("name")
                        ));
                        continue;
                    }
                },
            };
            let style = binding
                .kids(soap_ns, "binding")
                .next()
                .map(|b| b.attr("style").to_string())
                .unwrap_or_default();
            let port_type = port_types.get(local(binding.attr("type")));
            for op in binding.kids(WSDL_NS, "operation") {
                if endpoints.len() >= MAX_ENDPOINTS {
                    warnings.push(format!(
                        "Only the first {MAX_ENDPOINTS} operations were kept."
                    ));
                    break;
                }
                let op_name = op.attr("name");
                let soap_op = op.kids(soap_ns, "operation").next();
                let action = soap_op.map(|o| o.attr("soapAction")).unwrap_or_default();
                let op_style = soap_op
                    .map(|o| o.attr("style"))
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&style);
                let input = port_type
                    .and_then(|pt| {
                        pt.kids(WSDL_NS, "operation")
                            .find(|o| o.attr("name") == op_name)
                    })
                    .and_then(|o| o.kids(WSDL_NS, "input").next())
                    .and_then(|i| messages.get(local(i.attr("message"))));
                let mut params = Vec::new();
                let mut inner = String::new();
                let parts: Vec<&Node> = input
                    .map(|m| m.kids(WSDL_NS, "part").collect())
                    .unwrap_or_default();
                if op_style == "rpc" {
                    inner.push_str(&format!("<tns:{op_name}>"));
                    for part in &parts {
                        let name = part.attr("name");
                        let value = sample(local(part.attr("type")));
                        params.push(ApiParam {
                            name: name.to_string(),
                            location: ParamLocation::Body,
                            required: true,
                            example: value.clone(),
                        });
                        inner.push_str(&format!("<{name}>{value}</{name}>"));
                    }
                    inner.push_str(&format!("</tns:{op_name}>"));
                } else {
                    for part in &parts {
                        match schema.elements.get(local(part.attr("element"))) {
                            Some(element) => {
                                schema.write(element, "tns", 0, &mut inner, &mut params)
                            }
                            None => warnings.push(format!(
                                "{op_name}: element {} is not in the embedded schema.",
                                part.attr("element")
                            )),
                        }
                    }
                }
                let envelope_ns = if soap12 {
                    "http://www.w3.org/2003/05/soap-envelope"
                } else {
                    "http://schemas.xmlsoap.org/soap/envelope/"
                };
                let body = format!(
                    "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<soap:Envelope xmlns:soap=\"{envelope_ns}\" \
                     xmlns:tns=\"{}\">\n  <soap:Body>\n    {inner}\n  </soap:Body>\n</soap:Envelope>\n",
                    tns.replace('"', "&quot;")
                );
                let (content_type, headers) = if soap12 {
                    let mut ct = "application/soap+xml; charset=utf-8".to_string();
                    if !action.is_empty() {
                        ct.push_str(&format!("; action=\"{action}\""));
                    }
                    (ct, Vec::new())
                } else {
                    (
                        "text/xml; charset=utf-8".to_string(),
                        vec![("SOAPAction".to_string(), format!("\"{action}\""))],
                    )
                };
                endpoints.push(ApiEndpoint {
                    method: "POST".into(),
                    path: url::Url::parse(&url)
                        .map(|u| u.path().to_string())
                        .unwrap_or_default(),
                    url: url.clone(),
                    name: op_name.to_string(),
                    group: format!(
                        "{} ({})",
                        port.attr("name"),
                        if soap12 { "SOAP 1.2" } else { "SOAP 1.1" }
                    ),
                    params,
                    headers,
                    content_type: Some(content_type),
                    body: Some(body),
                    read_only: None,
                });
            }
        }
    }
    if endpoints.is_empty() && warnings.is_empty() {
        warnings.push("No SOAP ports were found.".into());
    }
    Ok(ApiDefinition {
        format: SpecFormat::Wsdl,
        title: defs.attr("name").to_string(),
        version: String::new(),
        endpoints,
        warnings,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // tests: panicking is the failure report
mod tests {
    use crate::{SpecFormat, parse, request_text};

    const WSDL: &str = r#"<?xml version="1.0"?>
<wsdl:definitions name="Users" targetNamespace="urn:users"
    xmlns:wsdl="http://schemas.xmlsoap.org/wsdl/"
    xmlns:soap="http://schemas.xmlsoap.org/wsdl/soap/"
    xmlns:soap12="http://schemas.xmlsoap.org/wsdl/soap12/"
    xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:tns="urn:users">
  <wsdl:types>
    <xs:schema targetNamespace="urn:users" elementFormDefault="qualified">
      <xs:element name="GetUser">
        <xs:complexType><xs:sequence>
          <xs:element name="id" type="xs:int"/>
          <xs:element name="filter" type="tns:Filter" minOccurs="0"/>
        </xs:sequence></xs:complexType>
      </xs:element>
      <xs:complexType name="Filter"><xs:sequence>
        <xs:element name="active" type="xs:boolean"/>
      </xs:sequence></xs:complexType>
    </xs:schema>
  </wsdl:types>
  <wsdl:message name="GetUserIn"><wsdl:part name="p" element="tns:GetUser"/></wsdl:message>
  <wsdl:portType name="UsersPort">
    <wsdl:operation name="GetUser"><wsdl:input message="tns:GetUserIn"/></wsdl:operation>
  </wsdl:portType>
  <wsdl:binding name="Soap11" type="tns:UsersPort">
    <soap:binding style="document" transport="http://schemas.xmlsoap.org/soap/http"/>
    <wsdl:operation name="GetUser"><soap:operation soapAction="urn:users#GetUser"/></wsdl:operation>
  </wsdl:binding>
  <wsdl:binding name="Soap12" type="tns:UsersPort">
    <soap12:binding style="document" transport="http://schemas.xmlsoap.org/soap/http"/>
    <wsdl:operation name="GetUser"><soap12:operation soapAction="urn:users#GetUser"/></wsdl:operation>
  </wsdl:binding>
  <wsdl:service name="UsersService">
    <wsdl:port name="P11" binding="tns:Soap11"><soap:address location="http://soap.test/users"/></wsdl:port>
    <wsdl:port name="P12" binding="tns:Soap12"><soap12:address location="/users12"/></wsdl:port>
  </wsdl:service>
</wsdl:definitions>"#;

    #[test]
    fn reads_soap_11_and_12_ports() {
        let def = parse(WSDL, None).unwrap();
        assert_eq!(def.format, SpecFormat::Wsdl);
        assert_eq!(def.title, "Users");
        assert_eq!(
            def.endpoints.len(),
            1,
            "the relative SOAP 1.2 port needs a base URL"
        );
        let e = &def.endpoints[0];
        assert_eq!(
            (e.method.as_str(), e.url.as_str()),
            ("POST", "http://soap.test/users")
        );
        assert_eq!(
            e.headers,
            vec![("SOAPAction".into(), "\"urn:users#GetUser\"".into())]
        );
        let body = e.body.as_deref().unwrap();
        assert!(
            body.contains(
                "<tns:GetUser><tns:id>1</tns:id><tns:filter><tns:active>true</tns:active></tns:filter></tns:GetUser>"
            ),
            "{body}"
        );
        assert!(body.contains("xmlns:soap=\"http://schemas.xmlsoap.org/soap/envelope/\""));
        assert!(request_text(e).contains("Content-Type: text/xml; charset=utf-8\r\n"));
        let def = parse(WSDL, Some("https://soap.test/")).unwrap();
        let p12 = &def.endpoints[1];
        assert_eq!(p12.url, "https://soap.test/users12");
        assert!(
            p12.content_type
                .as_deref()
                .unwrap()
                .contains("action=\"urn:users#GetUser\"")
        );
        assert!(
            p12.body
                .as_deref()
                .unwrap()
                .contains("http://www.w3.org/2003/05/soap-envelope")
        );
    }

    #[test]
    fn refuses_other_xml_and_survives_garbage() {
        assert!(
            parse("<html><body/></html>", None)
                .unwrap_err()
                .contains("WSDL")
        );
        assert!(parse("<a><b></a>", None).is_err());
        for garbage in ["<", "<a", "<?xml?>", "<x xmlns:=\"\"/>"] {
            let _ = parse(garbage, None);
        }
    }
}
