const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
// const { open } = window.__TAURI__.dialog;

const tauri = window.__TAURI__;

/** Send a message to the backend as JSON to be deserialized into the Rust enum BackendMessage. */
function sendBackendMessage(message) {
  console.log("Sending backend message", message);
  invoke("send_backend_message", {
    "message" : message
  }).then(result => {
    console.log("result of backend message: ", result);
  })
}

