import { FormEvent, useCallback, useEffect, useState } from "react";
import { PageProps, UnencryptedIdentity } from "../lib/types";
import Loader from "../components/Loader";
import { useNavigate } from "react-router-dom";
import { Tooltip } from "../components/Tooltip";
import { redirectToHomepage } from "../utils/redirect-to-homepage";

interface LoginProps extends PageProps { }

function Login({
  pw,
  setPw,
  routers,
  setRouters,
  hnsName,
  setHnsName,
}: LoginProps) {
  const navigate = useNavigate();

  const [keyErrs, setKeyErrs] = useState<string[]>([]);
  const [loading, setLoading] = useState<string>("");

  useEffect(() => {
    document.title = "Login";

    (async () => {
      try {
        const infoData = (await fetch("/info", { method: "GET", credentials: 'include' }).then((res) =>
          res.json()
        )) as UnencryptedIdentity;
        setRouters(infoData.allowed_routers);
        setHnsName(infoData.name);
      } catch { }
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleLogin = useCallback(
    async (e?: FormEvent) => {
      e?.preventDefault();
      e?.stopPropagation();

      setLoading("Logging in...");
      try {
        let result;

        try {
          // Try argon2 hash first
          const h = await argon2.hash({
            pass: pw,
            salt: hnsName,
            hashLen: 32,
            time: 2,
            mem: 19456,
            type: argon2.ArgonType.Argon2id
          });

          const hashed_password_hex = `0x${h.hashHex}`;

          result = await fetch("/login", {
            method: "POST",
            credentials: 'include',
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ password_hash: hashed_password_hex }),
          });

          if (result.status < 399) {
            redirectToHomepage();
            return;
          }
        } catch (argonErr) {
          console.log("This node was instantiated before the switch to argon2");
        }

        throw new Error(result ? await result.text() : "Login failed");

      } catch (err) {
        setKeyErrs([String(err)]);
        setLoading("");
      }
    },
    [pw]
  );

  const isDirect = Boolean(routers?.length === 0);

  return (
    <>
      {loading ? (
        <Loader msg={loading} />
      ) : (
        <form
          id="signup-form"
          className="form"
          onSubmit={handleLogin}
        >
          <div className="form-group">
            <div className="form-header">
              <Tooltip text={`(${isDirect ? "direct" : "indirect"} node)`}>
                <h3>{hnsName}</h3>
              </Tooltip>
            </div>
            <input
              type="password"
              id="password"
              required
              minLength={6}
              name="password"
              placeholder="Password"
              value={pw}
              onChange={(e) => setPw(e.target.value)}
              autoFocus
            />
          </div>

          {keyErrs.length > 0 && (
            <div className="error-messages">
              {keyErrs.map((x, i) => (
                <div key={i} className="error-message">{x}</div>
              ))}
            </div>
          )}

          <button type="submit">Log in</button>

          <div className="additional-options">
            <button
              className="secondary"
              onClick={() => navigate('/reset')}
            >
              Reset Password & Networking Info
            </button>
          </div>
        </form>
      )}
    </>
  );
}

export default Login;