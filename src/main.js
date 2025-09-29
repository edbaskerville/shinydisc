const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

let emailInput;
let pwInput;
let loginMsgEl;
let mfaInput;
let mfaMsgEl;

function setTab(targetTabName) {
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

function send_backend_message(message) {
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
  Request state update from backend
*/
function request_state() {
  send_backend_message({
    "RequestState" : null
  });
}

/*
  Listen to event updating state 
*/
listen('update-state', (event) => {
  console.log(
    "got update-state", event.payload
  );
  update_view_from_state(event.payload);

  // Code from command
  // let result = await invoke("init_view");
  // setTab(result.tab_name);
});

function update_view_from_state(state) {
  if(state["NeedsLogIn"]) {
    setTab("login");
  }
  else if(state["NeedsMfa"]) {
    setTab("mfa");
  }
  else if(state["LoggedIn"]) {
    setTab("loggedin");
  }
  else if(state["Syncing"]) {
    setTab("syncing");
  }
}

/*
  Send login information to backend
*/
function login() {
  send_backend_message({
    "LogIn" : {
      email: emailInput.value,
      password: pwInput.value
    }
  });
  // let result = invoke("login", { email: emailInput.value, password: pwInput.value });
  // console.log("login result:", result);
  // if(result.message) {
  //   loginMsgEl.textContent = result.message;
  // }
  // setTab(result.tab_name);
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


function login_mfa() {
  send_backend_message({
    "LogInMfa" : {
      "mfa_code": mfaInput.value
    }
  });
  // let result = invoke("login_mfa", { email: emailInput.value, password: pwInput.value, mfaCode: mfaInput.value });
  // console.log("login_mfa result:", result);
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

function sync() {
  let result = invoke("sync");
  console.log("sync result:", result);
}

listen('sync-update', (event) => {
  console.log(
    `got sync-update: ${event.payload}`
  );
});

window.addEventListener("DOMContentLoaded", () => {
  request_state();

  emailInput = document.querySelector("#email-input");
  pwInput = document.querySelector("#password-input");
  mfaInput = document.querySelector("#mfa-input");
  loginMsgEl = document.querySelector("#login-error-p");
  mfaMsgEl = document.querySelector("#mfa-error-p");

  document.querySelector("#test-backend-form").addEventListener("submit", (e) => {
    e.preventDefault();
    send_backend_message({
      "Test" : null
    });
  });

  document.querySelector("#login-form").addEventListener("submit", (e) => {
    e.preventDefault();
    login();
  });

  document.querySelector("#mfa-form").addEventListener("submit", (e) => {
    e.preventDefault();
    login_mfa();
  });

  document.querySelector("#loggedin-form").addEventListener("submit", (e) => {
    e.preventDefault();
    sync();
  });
});
