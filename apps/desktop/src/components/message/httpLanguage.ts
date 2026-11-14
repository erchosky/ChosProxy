import { StreamLanguage, type StringStream } from "@codemirror/language";

interface HttpState {
  line: number;
  inBody: boolean;
  /** Position inside a header line: before or after the colon. */
  afterColon: boolean;
}

/** Highlighting for raw HTTP/1.x message heads. The body is left plain. */
export const httpLanguage = StreamLanguage.define<HttpState>({
  name: "http",
  startState: () => ({ line: 0, inBody: false, afterColon: false }),
  blankLine(state) {
    state.inBody = true;
  },
  token(stream: StringStream, state: HttpState): string | null {
    if (stream.sol()) {
      state.afterColon = false;
      if (!state.inBody) state.line++;
    }
    if (state.inBody) {
      stream.skipToEnd();
      return null;
    }
    if (state.line === 1) {
      if (stream.match(/^HTTP\/\d(\.\d)?/)) return "meta";
      if (stream.match(/^\d{3}/)) return "number";
      if (stream.sol() && stream.match(/^[A-Z]+(?= )/)) return "keyword";
      if (stream.eatSpace()) return null;
      stream.match(/^\S+/);
      return "string";
    }
    if (!state.afterColon) {
      if (stream.match(/^[^:\s][^:]*/)) return "property";
      if (stream.eat(":")) {
        state.afterColon = true;
        return "punctuation";
      }
      stream.next();
      return null;
    }
    stream.skipToEnd();
    return "string";
  },
});
