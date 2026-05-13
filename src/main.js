const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
// const { open } = window.__TAURI__.dialog;

const tauri = window.__TAURI__;

let messageEl;
let outputDirEl;
let metadataCheckboxEl;
let gpsCoordsEl;
let cancelSyncBtn;
let state;

/** Switch tabs. */
export function setTab(targetTabName) {
  // TODO: Rewrite to eliminate knowledge of tabs
  for(let tabName of ["loggedout", "loggedin", "syncing"]) {
    let tabEl = document.querySelector("#" + tabName + "-tab");
    if(tabName == targetTabName) {
      tabEl.classList.add("tab-visible");
      tabEl.classList.remove("tab-hidden");
    }
    else {
      tabEl.classList.add("tab-hidden");
      tabEl.classList.remove("tab-visible");
    }
  }
}

/** Send a message to the backend as JSON to be deserialized into the Rust enum BackendMessage. */
export function sendBackendMessage(message) {
  console.log("Sending backend message");
  invoke("send_backend_message", {
    "message" : message
  });
}

/**
  Test event from Tauri backend
*/
listen('test-event', (event) => {
  console.log(
    `got test-event: ${event.payload}`
  );
  console.log(event.payload.message);
});

/*
  Listen to log messages from backend to be inserted into frontend console.
*/
listen('log-event', (event) => {
  console.log("log message from backend:", event.payload);
});

/*
  Listen to state updates from backend.
*/
listen('update-state', (event) => {
  console.log(
    "got update-state", event.payload
  );
  state = event.payload;
  updateViewFromState();
});

/**
  Manual view update based on backend state.
  
  Intentionally avoiding a proper reactive pure-functional thing.
*/
function updateViewFromState() {
  let backendState = state.backend_state;

  if(backendState["LoggedOut"]) {
    setTab("loggedout");
  }
  else if(backendState["LoggedIn"]) {
    setTab("loggedin");
  }
  else if(backendState["Syncing"]) {
    cancelSyncBtn.disabled = false;
    setTab("syncing");
  }
  else if(backendState["SyncCanceling"]) {
    console.log("state is SyncCanceling");
    cancelSyncBtn.disabled = true;
    setTab("syncing");
  }

  messageEl.innerText = state.message;
  metadataCheckboxEl.checked = state.update_all_metadata;
  outputDirEl.innerText = state.output_dir;

  if(gpsCoordsEl.value != state.gps_coords) {
    gpsCoordsEl.value = state.gps_coords;
  }
}

/**
  Set whether all files should have metadata (GPS coordinates) updated.
*/
function setUpdateAllMetadata() {
  sendBackendMessage({
    "SetUpdateAllMetadata" : Boolean(metadataCheckboxEl.checked)
  });
}

/**
  Set GPS coordinates to embed in photos and videos.
  
  This obviously ignores the fact that many photos are from field trips; those can be modified by parents as they desire in whatever tool they like.
*/
function setGPSCoords() {
  sendBackendMessage({
    "SetGPSCoords" : gpsCoordsEl.value
  });
}

/**
  Choose output path using the system file chooser.
*/
async function chooseOutputPath() {
  const output_dir = await tauri.dialog.open({
    multiple: false,
    directory: true,
    defaultPath: state.output_dir,
  });
  if(output_dir) {
    sendBackendMessage({
      "SetOutputDir" : output_dir
    });
  }
}

window.addEventListener("DOMContentLoaded", () => {
  sendBackendMessage({DOMContentLoaded: null});
  
  messageEl = document.querySelector("#message-p");
  outputDirEl = document.querySelector("#output-dir-p");
  metadataCheckboxEl = document.querySelector("#metadata-checkbox");
  gpsCoordsEl = document.querySelector("#gps-input");
  cancelSyncBtn = document.getElementById("cancel-sync-btn");
  // logOutBtn = document.getElementById("log-out-btn");
  console.log(cancelSyncBtn);

  metadataCheckboxEl.addEventListener("input", (e) => {
    setUpdateAllMetadata();
  });

  gpsCoordsEl.addEventListener("input", (e) => {
    setGPSCoords();
  });

  document.querySelector("#choose-folder-button").addEventListener("click", (e) => {
    e.preventDefault();
    chooseOutputPath();
  });

  document.querySelector("#loggedin-form").addEventListener("submit", (e) => {
    e.preventDefault();
    sendBackendMessage({Sync: null});
  });

  document.querySelector("#syncing-form").addEventListener("submit", (e) => {
    e.preventDefault();
    sendBackendMessage({CancelSync: null});
  });
});
