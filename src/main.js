const { invoke } = window.__TAURI__.core;

let emailInput;
let pwInput;
let loginMsgEl;

async function login() {
  // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
  console.log("i'm here");
  loginMsgEl.textContent = await invoke("login", { email: emailInput.value, password: pwInput.value });
  console.log("i'm here");
}

window.addEventListener("DOMContentLoaded", () => {
  emailInput = document.querySelector("#email-input");
  pwInput = document.querySelector("#password-input");
  loginMsgEl = document.querySelector("#login-msg");
  document.querySelector("#login-form").addEventListener("submit", (e) => {
    e.preventDefault();
    login();
  });
});
