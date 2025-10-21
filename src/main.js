const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
// const { open } = window.__TAURI__.dialog;

const tauri = window.__TAURI__;

let emailInput;
let pwInput;
let loginMsgEl;
let messageEl;
let outputDirEl;
let mfaInput;
let mfaMsgEl;
let cancelSyncBtn;
let logOutBtn;
let state;

export function setTab(targetTabName) {
  for(let tabName of ["login", "mfa", "loggedin", "syncing"]) {
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

export function sendBackendMessage(message) {
  console.log("Sending backend message");
  invoke("send_backend_message", {
    "message" : message
  });
}

/*
  Test event from Tauri backend
*/
listen('test-event', (event) => {
  console.log(
    `got test-event: ${event.payload}`
  );
  console.log(event.payload.message);
});

/*
  Listen to frontend
*/
listen('log-event', (event) => {
  console.log("log message from backend:", event.payload);
});

/*
  Listen to event updating state 
*/
listen('update-state', (event) => {
  console.log(
    "got update-state", event.payload
  );
  state = event.payload;
  updateViewFromState();

  // Code from command
  // let result = await invoke("init_view");
  // setTab(result.tab_name);
});

function updateViewFromState() {
  let backendState = state.backend_state;

  if(backendState["LoggedOut"]) {
    setTab("login");
  }
  else if(backendState["NeedsMfa"]) {
    setTab("mfa");
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
  outputDirEl.innerText = state.output_dir;
}

/*
  Send login information to backend
*/
function logIn() {
  sendBackendMessage({
    "LogIn" : {
      email: emailInput.value,
      password: pwInput.value
    }
  });
  pwInput.value = "";
}

/*
  Listen to login response from backend
*/
listen('login-response', (event) => {
  console.log(
    `got login-response: ${event.payload}`
  );

  // Code from command
  // let result = await invoke("init_view");
  // setTab(result.tab_name);
});


function logInMfa() {
  sendBackendMessage({
    "LogInMfa" : {
      "mfa_code": mfaInput.value
    }
  });
}

listen('login-mfa-response', (event) => {
  console.log(
    `got login-mfa-response: ${event.payload}`
  );
  // if(result.message) {
  //   mfaMsgEl.textContent = result.message;
  // }
  // setTab(result.tab_name);
});

/*
  Ask backend to log out
*/
function logOut() {
  sendBackendMessage({
    "LogOut" : null
  });
}

function chooseOutputPathSync() {
  chooseOutputPath().then(() => {
    console.log("promise succeeded");
  },
  () => {
    console.log("promise failed");
  });
}

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

listen('sync-update', (event) => {
  console.log(
    `got sync-update: ${event.payload}`
  );
});

window.addEventListener("DOMContentLoaded", () => {
  sendBackendMessage({DOMContentLoaded: null});

  emailInput = document.querySelector("#email-input");
  pwInput = document.querySelector("#password-input");
  mfaInput = document.querySelector("#mfa-input");
  loginMsgEl = document.querySelector("#login-error-p");
  messageEl = document.querySelector("#message-p");
  outputDirEl = document.querySelector("#output-dir-p")
  mfaMsgEl = document.querySelector("#mfa-error-p");
  cancelSyncBtn = document.getElementById("cancel-sync-btn");
  logOutBtn = document.getElementById("log-out-btn");
  console.log(cancelSyncBtn);

  logOutBtn.addEventListener("click", (e) => {
    logOut();
  });

  document.querySelector("#login-form").addEventListener("submit", (e) => {
    e.preventDefault();
    logIn();
  });

  document.querySelector("#mfa-form").addEventListener("submit", (e) => {
    e.preventDefault();
    logInMfa();
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
