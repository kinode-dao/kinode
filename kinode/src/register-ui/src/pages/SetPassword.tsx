import React, { useState, useEffect, FormEvent, useCallback } from "react";
import OsHeader from "../components/KnsHeader";
import Loader from "../components/Loader";
import { utils, providers } from "ethers";
import { downloadKeyfile } from "../utils/download-keyfile";
import { ReactComponent as NameLogo } from "../assets/kinode.svg"

type SetPasswordProps = {
  direct: boolean;
  pw: string;
  reset: boolean;
  provider?: providers.Web3Provider,
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
  provider,
  setPw,
  appSizeOnLoad,
  closeConnect,
  nodeChainId,
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
        let hashed_password = utils.sha256(utils.toUtf8Bytes(pw));
        let signer = await provider?.getSigner();
        let owner = await signer?.getAddress();
        let chain_id = await signer?.getChainId();
        let timestamp = Date.now();

        let sig_data = JSON.stringify({
          username: knsName,
          password_hash: hashed_password,
          timestamp,
          direct,
          reset,
          chain_id,
        });

        let signature = await signer?.signMessage(utils.toUtf8Bytes(sig_data));

        try {
          const result = await fetch("/boot", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            credentials: "include",
            body: JSON.stringify({
              password_hash: hashed_password,
              reset,
              username: knsName,
              direct,
              owner,
              timestamp,
              signature,
              chain_id,
            }),
          });
          const base64String = await result.json();

          downloadKeyfile(knsName, base64String);

          const interval = setInterval(async () => {
            const res = await fetch("/");

            if (
              res.status < 300 &&
              Number(res.headers.get("content-length")) !== appSizeOnLoad
            ) {
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
      <OsHeader
        header={<h3 className="row" style={{ justifyContent: "center", alignItems: "center" }}>
          Set
          <NameLogo style={{ height: 28, width: "auto", margin: "0 16px -3px" }} />
          Password
        </h3>}
        openConnect={() => { }}
        closeConnect={closeConnect}
        nodeChainId={nodeChainId}
      />
      {loading ? (
        <Loader msg="Setting up node..." />
      ) : (
        <form id="signup-form" className="col" onSubmit={handleSubmit}>
          <div style={{ width: "100%" }}>
            <div className="row label-row">
              <label htmlFor="password">New Password</label>
              <div className="tooltip-container">
                <div className="tooltip-button">&#8505;</div>
                <div className="tooltip-content">
                  This password will be used to log in if you restart your node
                  or switch browsers.
                </div>
              </div>
            </div>
            <input
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
          <div style={{ width: "100%" }}>
            <div className="row label-row">
              <label htmlFor="confirm-password">Confirm Password</label>
            </div>
            <input
              type="password"
              id="confirm-password"
              required
              minLength={6}
              name="confirm-password"
              placeholder="Min 6 characters"
              value={pw2}
              onChange={(e) => setPw2(e.target.value)}
            />
            {Boolean(error) && <p style={{ color: "red" }}>{error}</p>}
          </div>
          <button type="submit">Submit</button>
        </form>
      )}
    </>
  );
}

export default SetPassword;
