const { invoke } = window.__TAURI__.core;

let emailInput;
let pwInput;
let loginMsgEl;
let mfaInput;
let mfaMsgEl;

function setTab(targetTabName) {
  for(let tabName of ["login", "mfa", "loggedin"]) {
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

async function init_view() {
  let result = await invoke("init_view");
  setTab(result.tab_name);
}

async function login() {
  let result = await invoke("login", { email: emailInput.value, password: pwInput.value });
  console.log("login result:", result);
  if(result.message) {
    loginMsgEl.textContent = result.message;
  }
  setTab(result.tab_name);
}

async function login_mfa() {
  let result = await invoke("login_mfa", { email: emailInput.value, password: pwInput.value, mfaCode: mfaInput.value });
  console.log("login_mfa result:", result);
  if(result.message) {
    mfaMsgEl.textContent = result.message;
  }
  setTab(result.tab_name);
}

async function sync() {
  let result = await invoke("sync");
  console.log("sync result:", result);
}

window.addEventListener("DOMContentLoaded", () => {
  emailInput = document.querySelector("#email-input");
  pwInput = document.querySelector("#password-input");
  mfaInput = document.querySelector("#mfa-input");
  loginMsgEl = document.querySelector("#login-error-p");
  mfaMsgEl = document.querySelector("#mfa-error-p");
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

  init_view().then(() => {
    console.log("init_view returned");
  })
});
