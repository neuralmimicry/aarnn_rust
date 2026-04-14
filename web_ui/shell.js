const shellState = {
  authMode: "none",
  allowSignup: false,
  defaultOrchestrator: "",
  user: null,
  identity: null
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
function parseIdentityGroups(value) {
  if (!Array.isArray(value)) return [];
  const seen = new Set();
  return value.map(item => String(item || "").trim().toLowerCase()).filter(item => {
    if (!item || seen.has(item)) return false;
    seen.add(item);
    return true;
  });
}
function activeTeamLabel(value) {
  if (!value || typeof value !== "object") return "";
  return String(value.team_name || value.name || value.team_id || value.id || "").trim();
}
function normalizeIdentity(payload) {
  var _payload$team_count, _payload$pending_invi;
  if (!payload || payload.authenticated === false) return null;
  const username = String(payload.username || payload.user || "").trim();
  if (!username) return null;
  const role = String(payload.role || "user").trim().toLowerCase() || "user";
  const groups = parseIdentityGroups(payload.groups);
  if (!groups.includes(role)) {
    groups.unshift(role);
  }
  const activeTeam = payload.active_team && typeof payload.active_team === "object" ? payload.active_team : null;
  const teamCount = Math.max(0, Number((_payload$team_count = payload.team_count) !== null && _payload$team_count !== void 0 ? _payload$team_count : activeTeam ? 1 : 0) || 0);
  const pendingInvitationCount = Math.max(0, Number((_payload$pending_invi = payload.pending_invitation_count) !== null && _payload$pending_invi !== void 0 ? _payload$pending_invi : 0) || 0);
  return {
    username,
    role,
    groups,
    email: payload.email ? String(payload.email).trim() : null,
    activeTeam,
    activeTeamLabel: activeTeamLabel(activeTeam),
    teamCount,
    pendingInvitationCount,
    isAdmin: Boolean(payload.is_admin || role === "admin" || groups.includes("admin"))
  };
}
function applyShellIdentity(payload) {
  shellState.identity = normalizeIdentity(payload);
  shellState.user = shellState.identity ? shellState.identity.username : null;
}
function identitySummary(identity) {
  if (!identity) return "";
  const parts = [`Signed in as ${identity.username}`];
  if (identity.groups.length) {
    parts.push(`groups: ${identity.groups.join(", ")}`);
  }
  if (identity.activeTeamLabel) {
    parts.push(`team: ${identity.activeTeamLabel}`);
  }
  if (identity.teamCount > 1) {
    parts.push(`teams: ${identity.teamCount}`);
  }
  if (identity.pendingInvitationCount > 0) {
    parts.push(`invites: ${identity.pendingInvitationCount}`);
  }
  return parts.join(" | ");
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
    if (shellState.identity) {
      topbarStatus.textContent = identitySummary(shellState.identity);
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
    if (shellState.identity) {
      sessionState.textContent = identitySummary(shellState.identity);
    } else if (shellState.authMode === "none") {
      sessionState.textContent = "Anonymous access";
    } else {
      sessionState.textContent = "Signed out";
    }
  }
  if (defaultOrchestrator) {
    defaultOrchestrator.textContent = shellState.defaultOrchestrator || "Not configured";
  }
  renderRuntimeValue("shell-auth-mode", shellState.authMode, shellState.authMode === "none" ? "muted" : "ok");
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
    const [modeResp, configResp, meResp] = await Promise.all([fetch("/api/auth/mode").catch(() => null), fetch("/api/config").catch(() => null), fetch("/api/me").catch(() => null)]);
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
      applyShellIdentity(data);
    } else {
      applyShellIdentity(null);
    }
  } catch (_) {
    shellState.authMode = "none";
    applyShellIdentity(null);
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
      headers: {
        "Content-Type": "application/json"
      },
      body: JSON.stringify({
        username,
        password
      })
    });
    if (!resp.ok) {
      const data = await resp.json().catch(() => ({}));
      setShellError(data.error || "Login failed.");
      return;
    }
    const data = await resp.json();
    const nextPath = `${window.location.pathname || "/"}${window.location.search || ""}` || "/";
    if (submitAccessExchange(data === null || data === void 0 ? void 0 : data.access_token, nextPath)) {
      return;
    }
    applyShellIdentity(data);
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
      headers: {
        "Content-Type": "application/json"
      },
      body: JSON.stringify({
        username,
        password
      })
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
    await fetch("/api/logout", {
      method: "POST"
    });
  } catch (_) {}
  applyShellIdentity(null);
  syncShellUi();
}
function submitAccessExchange(accessToken, nextPath) {
  const token = String(accessToken || "").trim();
  if (!token) {
    return false;
  }
  const form = document.createElement("form");
  form.method = "POST";
  form.action = "/auth/access/exchange";
  form.style.display = "none";
  const fields = {
    access_token: token,
    next: nextPath || "/"
  };
  Object.entries(fields).forEach(([name, value]) => {
    const input = document.createElement("input");
    input.type = "hidden";
    input.name = name;
    input.value = value;
    form.appendChild(input);
  });
  document.body.appendChild(form);
  form.submit();
  return true;
}
function attachShellHandlers() {
  const loginForm = shellEl("shell-login-form");
  const logoutBtn = shellEl("shell-logout-btn");
  const signupBtn = shellEl("shell-signup-btn");
  const usernameEl = shellEl("shell-login-username");
  const passwordEl = shellEl("shell-login-password");
  if (loginForm && usernameEl && passwordEl) {
    loginForm.addEventListener("submit", event => {
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
