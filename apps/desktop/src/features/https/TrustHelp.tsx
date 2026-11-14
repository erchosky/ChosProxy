/** How to trust the CA manually, for browsers or tools not launched by ChosProxy. */
export function TrustHelp() {
  return (
    <section className="card">
      <h2>Trusting the CA manually</h2>
      <p className="muted">
        Not needed for the lab browser. Use this for another dedicated testing browser or for
        command-line tools. Only trust the CA where you inspect traffic you are authorized to test,
        and remove it when you are done.
      </p>
      <ul className="trust-steps">
        <li>
          <strong>Firefox (testing profile):</strong> browse to <code>http://chosproxy/</code>{" "}
          through the proxy, download the certificate, then Settings → Privacy &amp; Security →
          Certificates → View Certificates → Authorities → Import, and tick “Trust this CA to
          identify websites”.
        </li>
        <li>
          <strong>curl:</strong>{" "}
          <code>curl --proxy http://127.0.0.1:8080 --cacert chosproxy-ca.pem https://target</code>
        </li>
        <li>
          <strong>Kali system-wide (affects every program — avoid unless necessary):</strong> copy
          the PEM to <code>/usr/local/share/ca-certificates/chosproxy-ca.crt</code> and run{" "}
          <code>sudo update-ca-certificates</code>; remove it the same way afterwards.
        </li>
      </ul>
    </section>
  );
}
