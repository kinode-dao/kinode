import { FormEvent, useCallback, useEffect, useState } from "react";
import { PageProps, UnencryptedIdentity } from "../lib/types";
import Loader from "../components/Loader";
import { useNavigate } from "react-router-dom";
import { isMobileCheck } from "../utils/dimensions";
import classNames from "classnames";
import { getFetchUrl } from "../utils/fetch";
import { sha256, toBytes } from "viem";

interface LoginProps extends PageProps { }

function Login({
  pw,
  setPw,
  appSizeOnLoad,
  routers,
  setRouters,
  knsName,
  setOsName,
}: LoginProps) {
  const navigate = useNavigate();

  const [keyErrs, setKeyErrs] = useState<string[]>([]);
  const [loading, setLoading] = useState<string>("");

  useEffect(() => {
    document.title = "Login";

    (async () => {
      try {
        const infoData = (await fetch(getFetchUrl("/info"), { method: "GET", credentials: 'include' }).then((res) =>
          res.json()
        )) as UnencryptedIdentity;
        setRouters(infoData.allowed_routers);
        setOsName(infoData.name);
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
          getFetchUrl("login"),
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
          const res = await fetch(getFetchUrl("/"), { credentials: 'include' });
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
  const isMobile = isMobileCheck();

  return (
    <>
      {loading ? (
        <Loader msg={loading} />
      ) : (
        <form
          id="signup-form"
          className={classNames("flex flex-col w-full max-w-[450px]", {
            'p-2': isMobile
          })}
          onSubmit={handleLogin}
        >
          <div className="self-stretch mb-2 flex flex-col">
            <div className="flex text-lg mb-2 place-items-center place-content-center">
              <h3 className="font-bold">
                {knsName}
              </h3>
              <span className="ml-2 mt-1 text-sm">
                ({isDirect ? "direct" : "indirect"} node)
              </span>
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
              className="self-stretch"
            />
          </div>

          {keyErrs.length > 0 && <div className="flex flex-col w-full leading-6 mb-2">
            {keyErrs.map((x, i) => (
              <div key={i} className="text-red-500">
                {x}
              </div>
            ))}
          </div>}

          <button type="submit" className="w-full mb-2">Login</button>

          <div className="flex flex-col w-full self-stretch place-content-center place-items-center">
            <button
              className="clear self-stretch mb-1"
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