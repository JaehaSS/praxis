(function () {
  "use strict";
  var encoder = new TextEncoder();
  var empty = JSON.stringify({ probe: "strict-csp", bytes: "" });
  var bytes = "x".repeat(512 * 1024 - encoder.encode(empty).length);
  var payload = JSON.stringify({ probe: "strict-csp", bytes: bytes });
  var payloadBytes = encoder.encode(payload).length;
  if (payloadBytes !== 512 * 1024) throw new Error("strict-CSP payload must be exactly 512KiB");
  window.__praxisStrictCspProbe = { payload: payload, payloadBytes: payloadBytes };
})();
