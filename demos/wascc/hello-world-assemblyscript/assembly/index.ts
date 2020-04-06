import { handleCall, consoleLog, handleAbort } from "wapc-guest-as";
import { Environ, EnvironEntry } from "as-wasi";
import { Request, Response, ResponseBuilder, Handlers } from "./module";

export function _start(): void {
  Handlers.handleRequest(handleRequest);
}

function handleRequest(request: Request): Response {
  let message = "";
  let env = new Environ();
  let all_vars: Array<EnvironEntry> = env.all();

  for (var i = 0; i < all_vars.length - 1; i++) {
    message += all_vars[i].key + "=" + all_vars[i].value + "\n";
  }
  message += all_vars[all_vars.length-1].key + "=" + all_vars[all_vars.length-1].value + "\n";

  consoleLog(message);
  const payload = String.UTF8.encode(message);

  return new ResponseBuilder()
    .withStatusCode(200)
    .withStatus("OK")
    .withBody(payload)
    .build();
}

export function __guest_call(operation_size: usize, payload_size: usize): bool {
  return handleCall(operation_size, payload_size);
}
