import { useEffect, useState } from "react";
import { beginLogin, fetchMe, fetchProviders, logout, type AuthProvider, type AuthUser } from "./auth";
import { loadButterlogUserId, saveButterlogUserId } from "./persistence";
import "./AboutPage.css";
import "./SettingsPage.css";

/** Static `/settings` route (see main.tsx). Reuses the About page's card
 * chrome. Account settings only for now — a per-account home for the
 * preferences that arrive with Phase 4 sync; if you land here signed out,
 * it offers the sign-in buttons instead. No router: same single-path check
 * pattern as `/about`. */
export function SettingsPage() {
  const [user, setUser] = useState<AuthUser | null>(null);
  const [providers, setProviders] = useState<AuthProvider[]>([]);
  const [loading, setLoading] = useState(true);

  const [butterlogUserId, setButterlogUserId] = useState(() => loadButterlogUserId() || "");
  const [saved, setSaved] = useState(false);
  const [saveTimer, setSaveTimer] = useState<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([fetchMe(), fetchProviders()]).then(([u, p]) => {
      if (cancelled) return;
      setUser(u);
      setProviders(p);
      setLoading(false);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    return () => {
      if (saveTimer) clearTimeout(saveTimer);
    };
  }, [saveTimer]);

  const handleButterlogUserIdChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const val = e.target.value;
    setButterlogUserId(val);
    saveButterlogUserId(val);
    setSaved(true);
    if (saveTimer) clearTimeout(saveTimer);
    const timer = setTimeout(() => {
      setSaved(false);
    }, 1500);
    setSaveTimer(timer);
  };

  return (
    <div className="about-page">
      <div className="about-card">
        <h1>Settings</h1>
        
        <div className="settings-section">
          {loading ? (
            <p>Loading…</p>
          ) : user ? (
            <>
              <div className="settings-account">
                {user.avatar_url && (
                  <img className="settings-avatar" src={user.avatar_url} alt="" width={48} height={48} />
                )}
                <div>
                  <div className="settings-name">{user.name}</div>
                  {user.email && <div className="settings-email">{user.email}</div>}
                  <div className="settings-provider">Signed in with {user.provider}</div>
                </div>
              </div>
              <p>
                Account preferences and cross-device sync of your flight plans and aircraft profiles are
                coming soon. For now, signing in just remembers who you are.
              </p>
              <button
                className="settings-logout"
                onClick={() => {
                  logout().finally(() => setUser(null));
                }}
              >
                Log out
              </button>
            </>
          ) : providers.length > 0 ? (
            <>
              <p>Sign in to manage your account settings.</p>
              <div className="settings-login-buttons">
                {providers.map((p) => (
                  <button key={p.id} onClick={() => beginLogin(p.id)}>
                    Log in with {p.display_name}
                  </button>
                ))}
              </div>
            </>
          ) : (
            <p>Sign-in isn't enabled on this deployment.</p>
          )}
        </div>

        <div className="settings-divider" />

        <div className="settings-section">
          <h2>Integrations</h2>
          <p className="settings-help">
            Enter your Butterlog User ID to display your current flight simulator location and flight telemetry on the map.
          </p>
          <div className="settings-form-group">
            <label htmlFor="butterlog-userid">Butterlog User ID</label>
            <div className="settings-input-wrapper">
              <input
                id="butterlog-userid"
                type="text"
                value={butterlogUserId}
                onChange={handleButterlogUserIdChange}
                placeholder="e.g. 12345"
              />
              <span className={`settings-saved-indicator ${saved ? "visible" : ""}`}>
                Saved
              </span>
            </div>
          </div>
        </div>

        <a className="about-back" href="/">
          ← Back to the app
        </a>
      </div>
    </div>
  );
}
