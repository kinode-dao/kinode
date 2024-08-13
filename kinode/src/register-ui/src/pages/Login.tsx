import { FormEvent, useCallback, useEffect, useState } from "react";
import { PageProps, UnencryptedIdentity } from "../lib/types";
import Loader from "../components/Loader";
import { useNavigate } from "react-router-dom";
import { sha256, toBytes } from "viem";

interface LoginProps extends PageProps { }

function Login({
  pw,
  setPw,
  appSizeOnLoad,
  routers,
  setRouters,
  knsName,
  setKnsName,
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
        setKnsName(infoData.name);
      } catch { }
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleLogin = useCallback(
    async (e?: FormEvent) => {
      e?.preventDefault();
      e?.stopPropagation();

      try {
        setLoading("Logging in...");
        let hashed_password = sha256(toBytes(pw));

        const result = await fetch(
          "/login",
          {
            method: "POST",
            credentials: 'include',
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ password_hash: hashed_password }),
          }
        );

        if (result.status > 399) {
          throw new Error(await result.text());
        }

        const interval = setInterval(async () => {
          const res = await fetch("/", { credentials: 'include' });
          if (
            res.status < 300 &&
            Number(res.headers.get("content-length")) !== appSizeOnLoad
          ) {
            clearInterval(interval);
            window.location.replace("/");
          }
        }, 2000);
      } catch (err: any) {
        setKeyErrs([String(err)]);
        setLoading("");
      }
    },
    [pw, appSizeOnLoad]
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
              <h3>{knsName}</h3>
              <span>({isDirect ? "direct" : "indirect"} node)</span>
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

          <button type="submit">Login</button>

          <div className="additional-options">
            <button
              className="clear"
              onClick={() => navigate('/reset')}
            >
              Reset Node & Networking Info
            </button>
          </div>
        </form>
      )}
    </>
  );
}

export default Login;