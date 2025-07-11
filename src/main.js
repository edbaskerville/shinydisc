const { invoke } = window.__TAURI__.core;

let studentInputEl;
let loginMsgEl;

async function login() {
  // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
  loginMsgEl.textContent = await invoke("login", { name: studentInputEl.value });
}

window.addEventListener("DOMContentLoaded", () => {
  studentInputEl = document.querySelector("#student-input");
  loginMsgEl = document.querySelector("#login-msg");
  document.querySelector("#login-form").addEventListener("submit", (e) => {
    e.preventDefault();
    login();
  });
});
