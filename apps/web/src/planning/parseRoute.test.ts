import { cleanRouteTokens, isRouteString } from "./parseRoute.ts";

function assertEqual<T>(actual: T, expected: T, message?: string) {
  const a = JSON.stringify(actual);
  const b = JSON.stringify(expected);
  if (a !== b) {
    throw new Error(`${message ?? "Assertion failed"}: ${a} !== ${b}`);
  }
}

// 1. User's exact prompt route
const userExample =
  "KMMU/05 N0173F040 DCT PIYIN V3 SBJ V419 BIGGY V3 MXE V419 EMI V166 MRB/N0178F060 V44 MGW V75 AIR V103 ATWOO DCT KMNN/13";
const tokens = cleanRouteTokens(userExample);
assertEqual(tokens, [
  "KMMU",
  "PIYIN",
  "V3",
  "SBJ",
  "V419",
  "BIGGY",
  "V3",
  "MXE",
  "V419",
  "EMI",
  "V166",
  "MRB",
  "V44",
  "MGW",
  "V75",
  "AIR",
  "V103",
  "ATWOO",
  "KMNN",
]);
assertEqual(isRouteString(userExample), true);

// 2. Enclosed in quotes
assertEqual(cleanRouteTokens(`"${userExample}"`), tokens);
assertEqual(cleanRouteTokens(`'${userExample}'`), tokens);

// 3. Dot separators (SimBrief style)
assertEqual(cleanRouteTokens("KMMU..PIYIN.V3.SBJ..KMNN"), ["KMMU", "PIYIN", "V3", "SBJ", "KMNN"]);
assertEqual(isRouteString("KMMU..PIYIN.V3.SBJ..KMNN"), true);

// 4. Comma separators
assertEqual(cleanRouteTokens("KMMU, PIYIN, V3, SBJ, KMNN"), ["KMMU", "PIYIN", "V3", "SBJ", "KMNN"]);

// 5. Slashes with surrounding spaces
assertEqual(cleanRouteTokens("KMMU / 05 PIYIN V3 SBJ / VFR KMNN / 13"), ["KMMU", "PIYIN", "V3", "SBJ", "KMNN"]);

// 6. Direct routing and flight rules
assertEqual(cleanRouteTokens("KMMU IFR DCT PIYIN DIRECT BIGGY VFR KMNN"), ["KMMU", "PIYIN", "BIGGY", "KMNN"]);

// 7. Various speed / altitude qualifiers
assertEqual(cleanRouteTokens("KMMU N0150A045 PIYIN M082F370 SBJ FL040 BIGGY 4000FT KMNN"), [
  "KMMU",
  "PIYIN",
  "SBJ",
  "BIGGY",
  "KMNN",
]);

// 8. Single token vs multi-token route detection
assertEqual(isRouteString("SBJ"), false);
assertEqual(isRouteString("V3"), false);
assertEqual(isRouteString("PIYIN"), false);
assertEqual(isRouteString(""), false);
assertEqual(isRouteString("   "), false);

// 9. Single token with runway / route slash
assertEqual(isRouteString("KMMU/05"), true);
assertEqual(cleanRouteTokens("KMMU/05"), ["KMMU"]);

// 10. Multi-token partial routes
assertEqual(isRouteString("PIYIN V3 SBJ"), true);
assertEqual(cleanRouteTokens("PIYIN V3 SBJ"), ["PIYIN", "V3", "SBJ"]);

console.log("parseRoute.test.ts: All 10 tests passed!");
