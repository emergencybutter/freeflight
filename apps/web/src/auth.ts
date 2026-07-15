// Client half of the OAuth sign-in flow (see services/ff-api/src/routes/auth.rs).
//
// The session is an opaque bearer token minted by ff-api. Login is a
// top-level navigation to ff-api's /auth/login/:provider; the callback
// redirects back here with the token in the URL *fragment*
// (`#ff_auth=…`), which `captureAuthFromHash` below reads once on load and
// stashes in localStorage. Everything after that replays it as an
// `Authorization: Bearer` header — no cookies, so it works whether the SPA
// is same-origin with ff-api (prod) or cross-origin (Vite dev on :5173).
import { API_BASE_URL } from "./api";

const TOKEN_KEY = "ff_auth_token";

export interface AuthUser {
  provider: "google" | "discord" | string;
  subject: string;
  name: string;
  email: string | null;
  avatar_url: string | null;
}

export interface AuthProvider {
  id: string;
  display_name: string;
}

export function getToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    // Private-mode / storage-blocked browsers: treat as signed out.
    return null;
  }
}

function setToken(token: string): void {
  try {
    localStorage.setItem(TOKEN_KEY, token);
  } catch {
    /* ignore — session just won't persist */
  }
}

function clearToken(): void {
  try {
    localStorage.removeItem(TOKEN_KEY);
  } catch {
    /* ignore */
  }
}

/**
 * Reads a freshly-issued session token (or an error marker) out of the URL
 * fragment left by ff-api's OAuth callback, persists it, and scrubs the
 * fragment so a token never lingers in the address bar or history state.
 * Returns an error code if the provider/handshake reported one. Call once,
 * early, on app start.
 */
export function captureAuthFromHash(): { error?: string } {
  const hash = window.location.hash;
  if (!hash || hash.length < 2) return {};
  const params = new URLSearchParams(hash.slice(1));
  const token = params.get("ff_auth");
  const error = params.get("ff_auth_error") ?? undefined;
  if (!token && !error) return {};

  if (token) setToken(token);
  // Strip only our own keys; leave any unrelated fragment intact.
  params.delete("ff_auth");
  params.delete("ff_auth_error");
  const rest = params.toString();
  const newHash = rest ? `#${rest}` : "";
  window.history.replaceState(
    null,
    "",
    window.location.pathname + window.location.search + newHash,
  );
  return { error };
}

/** Providers this deployment has configured. Empty ⇒ sign-in is off. */
export async function fetchProviders(): Promise<AuthProvider[]> {
  try {
    const res = await fetch(`${API_BASE_URL}/auth/providers`);
    if (!res.ok) return [];
    return (await res.json()) as AuthProvider[];
  } catch {
    return [];
  }
}

/**
 * The current signed-in user, or null if the token is missing/expired/
 * rejected (a stale token is cleared so we don't keep retrying it).
 */
export async function fetchMe(): Promise<AuthUser | null> {
  const token = getToken();
  if (!token) return null;
  try {
    const res = await fetch(`${API_BASE_URL}/auth/me`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (res.status === 401) {
      clearToken();
      return null;
    }
    if (!res.ok) return null;
    return (await res.json()) as AuthUser;
  } catch {
    return null;
  }
}

/** Begins sign-in with a provider — a full-page navigation to ff-api. */
export function beginLogin(provider: string): void {
  const params = new URLSearchParams({ return: window.location.origin });
  window.location.assign(`${API_BASE_URL}/auth/login/${provider}?${params}`);
}

/** Revokes the session server-side (best-effort) and locally. */
export async function logout(): Promise<void> {
  const token = getToken();
  clearToken();
  if (!token) return;
  try {
    await fetch(`${API_BASE_URL}/auth/logout`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
    });
  } catch {
    /* already cleared locally; server entry expires on its own */
  }
}
