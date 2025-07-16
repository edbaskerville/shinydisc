const { invoke } = window.__TAURI__.core;

let emailInput;
let pwInput;
let loginMsgEl;

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

async function login() {
  // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
  let loginResult = await invoke("login", { email: emailInput.value, password: pwInput.value });
  if(loginResult.message) {
    loginMsgEl.textContent = loginResult.message;
  }
  setTab(loginResult.tab_name);
}

window.addEventListener("DOMContentLoaded", () => {
  emailInput = document.querySelector("#email-input");
  pwInput = document.querySelector("#password-input");
  loginMsgEl = document.querySelector("#login-error-p");
  document.querySelector("#login-form").addEventListener("submit", (e) => {
    e.preventDefault();
    login();
  });
});
