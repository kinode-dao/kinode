import React, { useState, useEffect, FormEvent, useCallback } from "react";
import Loader from "../components/Loader";
import { downloadKeyfile } from "../utils/download-keyfile";
import { Tooltip } from "../components/Tooltip";
import { useSignTypedData, useAccount, useChainId } from 'wagmi'
import { HYPERMAP } from "../abis";
import { redirectToHomepage } from "../utils/redirect-to-homepage";

type SetPasswordProps = {
  direct: boolean;
  pw: string;
  reset: boolean;
  hnsName: string;
  setPw: React.Dispatch<React.SetStateAction<string>>;
  nodeChainId: string;
  closeConnect: () => void;
};

function SetPassword({
  hnsName,
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
        argon2.hash({ pass: pw, salt: hnsName, hashLen: 32, time: 2, mem: 19456, type: argon2.ArgonType.Argon2id }).then(async h => {
          const hashed_password_hex = `0x${h.hashHex}` as `0x${string}`;
          let owner = address;
          let timestamp = Date.now();

          const signature = await signTypedDataAsync({
            domain: {
              name: "Hypermap",
              version: "1",
              chainId: chainId,
              verifyingContract: HYPERMAP,
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
              username: hnsName,
              password_hash: hashed_password_hex,
              timestamp: BigInt(timestamp),
              direct,
              reset,
              chain_id: BigInt(chainId),
            },
          });

          try {
            const result = await fetch("/boot", {
              method: "POST",
              headers: { "Content-Type": "application/json" },
              credentials: "include",
              body: JSON.stringify({
                password_hash: hashed_password_hex,
                reset,
                username: hnsName,
                direct,
                owner,
                timestamp,
                signature,
                chain_id: chainId,
              }),
            });
            const base64String = await result.json();

            downloadKeyfile(hnsName, base64String);
            redirectToHomepage();

          } catch {
            alert("There was an error setting your password, please try again.");
            setLoading(false);
          }
        }).catch(err => {
          alert(String(err));
          setLoading(false);
        });
      }, 500);
    },
    [direct, pw, pw2, reset, hnsName]
  );

  return (
    <>
      {loading ? (
        <Loader msg="Please sign the structured message in your wallet to set your password." />
      ) : (
        <form className="form" onSubmit={handleSubmit}>
          <div className="form-group">
            <Tooltip text="This password will be used to log in when you restart your node or switch browsers.">
              <label className="form-label" htmlFor="password">Set password for {hnsName}</label>
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