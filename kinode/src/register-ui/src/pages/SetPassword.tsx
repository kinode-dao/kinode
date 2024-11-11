import React, { useState, useEffect, FormEvent, useCallback } from "react";
import Loader from "../components/Loader";
import { downloadKeyfile } from "../utils/download-keyfile";
import { Tooltip } from "../components/Tooltip";
import { sha256, toBytes } from "viem";
import { useSignTypedData, useAccount, useChainId } from 'wagmi'
import { KIMAP } from "../abis";
import { redirectToHomepage } from "../utils/redirect-to-homepage";

type SetPasswordProps = {
  direct: boolean;
  pw: string;
  reset: boolean;
  knsName: string;
  setPw: React.Dispatch<React.SetStateAction<string>>;
  nodeChainId: string;
  closeConnect: () => void;
};

function SetPassword({
  knsName,
  direct,
  pw,
  reset,
  setPw,
}: SetPasswordProps) {
  const [pw2, setPw2] = useState("");
  const [error, setError] = useState("");
  const [loading, setLoading] = useState<boolean>(false);

  const { signTypedDataAsync } = useSignTypedData();
  const { address } = useAccount();
  const chainId = useChainId();

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
        let salted = [knsName, pw].join("");
        let hashed_password = sha256(toBytes(salted));
        let owner = address;
        let timestamp = Date.now();

        const signature = await signTypedDataAsync({
          domain: {
            name: "Kimap",
            version: "1",
            chainId: chainId,
            verifyingContract: KIMAP,
          },
          types: {
            Boot: [
              { name: 'username', type: 'string' },
              { name: 'password_hash', type: 'bytes32' },
              { name: 'timestamp', type: 'uint256' },
              { name: 'direct', type: 'bool' },
              { name: 'reset', type: 'bool' },
              { name: 'chain_id', type: 'uint256' },
            ],
          },
          primaryType: 'Boot',
          message: {
            username: knsName,
            password_hash: hashed_password,
            timestamp: BigInt(timestamp),
            direct,
            reset,
            chain_id: BigInt(chainId),
          },
        })

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
              chain_id: chainId,
            }),
          });
          const base64String = await result.json();

          downloadKeyfile(knsName, base64String);
          redirectToHomepage();

        } catch {
          alert("There was an error setting your password, please try again.");
          setLoading(false);
        }
      }, 500);
    },
    [direct, pw, pw2, reset, knsName]
  );

  return (
    <>
      {loading ? (
        <Loader msg="Please sign the structured message in your wallet to set your password." />
      ) : (
        <form className="form" onSubmit={handleSubmit}>
          <div className="form-group">
            <Tooltip text="This password will be used to log in when you restart your node or switch browsers.">
              <label className="form-label" htmlFor="password">Set password for {knsName}</label>
            </Tooltip>
            <input
              type="password"
              id="password"
              required
              minLength={6}
              name="password"
              placeholder="6 characters minimum"
              value={pw}
              onChange={(e) => setPw(e.target.value)}
              autoFocus
            />
          </div>
          <div className="form-group">
            <label className="form-label" htmlFor="confirm-password">Confirm Password</label>
            <input
              type="password"
              id="confirm-password"
              required
              minLength={6}
              name="confirm-password"
              placeholder="6 characters minimum"
              value={pw2}
              onChange={(e) => setPw2(e.target.value)}
            />
          </div>
          {Boolean(error) && <p className="error-message">{error}</p>}
          <button type="submit" className="button">Submit</button>
        </form>
      )}
    </>
  );
}

export default SetPassword;