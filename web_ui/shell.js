const shellState = {
  authMode: "none",
  allowSignup: false,
  defaultOrchestrator: "",
  user: null,
};

function shellEl(id) {
  return document.getElementById(id);
}

function setShellError(message = "") {
  const errorEl = shellEl("shell-auth-error");
  if (errorEl) {
    errorEl.textContent = message;
  }
}

function renderRuntimeValue(id, value, tone = "muted") {
  const el = shellEl(id);
  if (!el) return;
  el.innerHTML = `<span class="status-badge ${tone}">${value}</span>`;
}

function syncShellUi() {
  const topbarStatus = shellEl("shell-user-status");
  const logoutBtn = shellEl("shell-logout-btn");
  const sessionState = shellEl("shell-session-state");
  const loginForm = shellEl("shell-login-form");
  const signupBtn = shellEl("shell-signup-btn");
  const oidcLink = shellEl("shell-oidc-link");
  const defaultOrchestrator = shellEl("shell-default-orchestrator");

  if (topbarStatus) {
    if (shellState.user) {
      topbarStatus.textContent = `Signed in as ${shellState.user}`;
    } else if (shellState.authMode === "none") {
      topbarStatus.textContent = "No auth required";
    } else {
      topbarStatus.textContent = `Auth: ${shellState.authMode}`;
    }
  }

  if (logoutBtn) {
    logoutBtn.classList.toggle("hidden", !shellState.user);
  }

  if (sessionState) {
    if (shellState.user) {
      sessionState.textContent = `Signed in as ${shellState.user}`;
    } else if (shellState.authMode === "none") {
      sessionState.textContent = "Anonymous access";
    } else {
      sessionState.textContent = "Signed out";
    }
  }

  if (defaultOrchestrator) {
    defaultOrchestrator.textContent = shellState.defaultOrchestrator || "Not configured";
  }

  renderRuntimeValue(
    "shell-auth-mode",
    shellState.authMode,
    shellState.authMode === "none" ? "muted" : "ok"
  );

  if (!loginForm) return;
  const showLocalForm = shellState.authMode === "local" && !shellState.user;
  const showOidc = shellState.authMode === "oidc" && !shellState.user;
  loginForm.classList.toggle("hidden", !showLocalForm);
  if (signupBtn) {
    signupBtn.classList.toggle("hidden", !showLocalForm || !shellState.allowSignup);
  }
  if (oidcLink) {
    oidcLink.classList.toggle("hidden", !showOidc);
  }
}

async function loadShellRuntime() {
  try {
    const [modeResp, configResp, meResp] = await Promise.all([
      fetch("/api/auth/mode").catch(() => null),
      fetch("/api/config").catch(() => null),
      fetch("/api/me").catch(() => null),
    ]);

    if (modeResp && modeResp.ok) {
      const data = await modeResp.json();
      shellState.authMode = data.mode || "none";
      shellState.allowSignup = Boolean(data.allow_signup);
    }

    if (configResp && configResp.ok) {
      const data = await configResp.json();
      shellState.defaultOrchestrator = data.default_orchestrator || "";
    }

    if (meResp && meResp.ok) {
      const data = await meResp.json();
      shellState.user = data.username || null;
    } else {
      shellState.user = null;
    }
  } catch (_) {
    shellState.authMode = "none";
    shellState.user = null;
  }

  syncShellUi();
}

async function shellLogin(username, password) {
  if (!username || !password) {
    setShellError("Enter username and password.");
    return;
  }
  setShellError("");
  try {
    const resp = await fetch("/api/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    if (!resp.ok) {
      const data = await resp.json().catch(() => ({}));
      setShellError(data.error || "Login failed.");
      return;
    }
    const data = await resp.json();
    shellState.user = data.username || username;
    syncShellUi();
  } catch (_) {
    setShellError("Login failed.");
  }
}

async function shellSignup(username, password) {
  if (!username || !password) {
    setShellError("Enter username and password.");
    return;
  }
  setShellError("");
  try {
    const resp = await fetch("/api/signup", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ username, password }),
    });
    if (!resp.ok) {
      const data = await resp.json().catch(() => ({}));
      setShellError(data.error || "Signup failed.");
      return;
    }
    setShellError("Signup successful. Log in with the new account.");
  } catch (_) {
    setShellError("Signup failed.");
  }
}

async function shellLogout() {
  setShellError("");
  try {
    await fetch("/api/logout", { method: "POST" });
  } catch (_) {}
  shellState.user = null;
  syncShellUi();
}

function attachShellHandlers() {
  const loginForm = shellEl("shell-login-form");
  const logoutBtn = shellEl("shell-logout-btn");
  const signupBtn = shellEl("shell-signup-btn");
  const usernameEl = shellEl("shell-login-username");
  const passwordEl = shellEl("shell-login-password");

  if (loginForm && usernameEl && passwordEl) {
    loginForm.addEventListener("submit", (event) => {
      event.preventDefault();
      shellLogin(usernameEl.value.trim(), passwordEl.value);
    });
  }

  if (signupBtn && usernameEl && passwordEl) {
    signupBtn.addEventListener("click", () => {
      shellSignup(usernameEl.value.trim(), passwordEl.value);
    });
  }

  if (logoutBtn) {
    logoutBtn.addEventListener("click", () => {
      shellLogout();
    });
  }
}

document.addEventListener("DOMContentLoaded", () => {
  attachShellHandlers();
  loadShellRuntime();
});
