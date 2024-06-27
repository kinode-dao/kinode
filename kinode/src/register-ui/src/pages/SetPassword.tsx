import React, { useState, useEffect, FormEvent, useCallback } from "react";
import Loader from "../components/Loader";
import { downloadKeyfile } from "../utils/download-keyfile";
import { Tooltip } from "../components/Tooltip";
import { getFetchUrl } from "../utils/fetch";

import { sha256, toBytes } from "viem";

type SetPasswordProps = {
  direct: boolean;
  pw: string;
  reset: boolean;
  knsName: string;
  setPw: React.Dispatch<React.SetStateAction<string>>;
  appSizeOnLoad: number;
  nodeChainId: string;
  closeConnect: () => void;
};

function SetPassword({
  knsName,
  direct,
  pw,
  reset,
  setPw,
  appSizeOnLoad,
}: SetPasswordProps) {
  const [pw2, setPw2] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState<boolean>(false);

  useEffect(() => {
    document.title = "Set Password";
  }, []);

  useEffect(() => {
    setError("");
  }, [pw, pw2]);

  const handleSubmit = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();

      if (pw !== pw2) {
        setError("Passwords do not match");
        return false;
      }

      setTimeout(async () => {
        setLoading(true);
        let hashed_password = sha256(toBytes(pw));
        // let owner = await signer?.getAddress();
        // let chain_id = await signer?.getChainId();
        // let timestamp = Date.now();

        // let sig_data = JSON.stringify({
        //   username: knsName,
        //   password_hash: hashed_password,
        //   timestamp,
        //   direct,
        //   reset,
        //   chain_id,
        // });

        // let signature = await signer?.signMessage(utils.toUtf8Bytes(sig_data));

        try {
          const result = await fetch(getFetchUrl("/boot"), {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            credentials: "include",
            body: JSON.stringify({
              password_hash: hashed_password,
              reset,
              username: knsName,
              direct,
              // owner,
              // timestamp,
              // signature,
              // chain_id,
            }),
          });
          const base64String = await result.json();

          downloadKeyfile(knsName, base64String);

          const interval = setInterval(async () => {
            const res = await fetch(getFetchUrl("/"), { credentials: 'include' });

            if (
              res.status < 300 &&
              Number(res.headers.get("content-length")) !== appSizeOnLoad
            ) {
              console.log("WE GOOD, ROUTING")
              clearInterval(interval);
              window.location.replace("/");
            }
          }, 2000);
        } catch {
          alert("There was an error setting your password, please try again.");
          setLoading(false);
        }
      }, 500);
    },
    [appSizeOnLoad, direct, pw, pw2, reset, knsName]
  );

  return (
    <>

      {loading ? (
        <Loader msg="Setting up node..." />
      ) : (
        <form id="signup-form" className="flex flex-col w-full max-w-[450px] gap-4" onSubmit={handleSubmit}>
          <div className="flex flex-col w-full place-items-center place-content-center">
            <div className="flex w-full place-items-center mb-2">
              <label className="flex leading-6 place-items-center mt-2 cursor-pointer mb-2" style={{ fontSize: 20 }} htmlFor="password">New Password</label>
              <Tooltip text={`This password will be used to log in if you restart your node or switch browsers.`} />
            </div>
            <div className="flex w-full place-items-center">
              <input
                className="grow"
                type="password"
                id="password"
                required
                minLength={6}
                name="password"
                placeholder="Min 6 characters"
                value={pw}
                onChange={(e) => setPw(e.target.value)}
                autoFocus
              />
            </div>
          </div>
          <div className="flex flex-col w-full place-items-center place-content-center">
            <div className="flex w-full place-items-center">
              <label className="flex leading-6 place-items-center mt-2 cursor-pointer mb-4" style={{ fontSize: 20 }} htmlFor="confirm-password">Confirm Password</label>
            </div>
            <div className="flex w-full place-items-center">
              <input
                className="grow"
                type="password"
                id="confirm-password"
                required
                minLength={6}
                name="confirm-password"
                placeholder="Min 6 characters"
                value={pw2}
                onChange={(e) => setPw2(e.target.value)}
              />
            </div>
            {Boolean(error) && <p style={{ color: "red" }}>{error}</p>}
          </div>
          <button type="submit">Submit</button>
        </form>
      )}
    </>
  );
}

export default SetPassword;
