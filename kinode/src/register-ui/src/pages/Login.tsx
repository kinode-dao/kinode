import { FormEvent, useCallback, useEffect, useState } from "react";
import { namehash } from "ethers/lib/utils";
import { BytesLike, utils } from "ethers";
import KinodeHeader from "../components/KnsHeader";
import { NetworkingInfo, PageProps, UnencryptedIdentity } from "../lib/types";
import Loader from "../components/Loader";
import { hooks } from "../connectors/metamask";
import { ipToNumber } from "../utils/ipToNumber";
import { downloadKeyfile } from "../utils/download-keyfile";
import DirectCheckbox from "../components/DirectCheckbox";
import { useNavigate } from "react-router-dom";
import { Tooltip } from "../components/Tooltip";
import { KinodeTitle } from "../components/KinodeTitle";
import { isMobileCheck } from "../utils/dimensions";
import classNames from "classnames";

const { useProvider } = hooks;

interface LoginProps extends PageProps { }

function Login({
  direct,
  setDirect,
  pw,
  setPw,
  kns,
  openConnect,
  appSizeOnLoad,
  closeConnect,
  routers,
  setRouters,
  knsName,
  setOsName,
  nodeChainId,
}: LoginProps) {
  const provider = useProvider();
  const navigate = useNavigate()

  const [keyErrs, setKeyErrs] = useState<string[]>([]);
  const [loading, setLoading] = useState<string>("");
  const [showReset, setShowReset] = useState<boolean>(false);
  const [reset, setReset] = useState<boolean>(false);
  const [_restartFlow, setRestartFlow] = useState<boolean>(false);

  useEffect(() => {
    document.title = "Login";

    (async () => {
      try {
        const infoData = (await fetch("/info", { method: "GET" }).then((res) =>
          res.json()
        )) as UnencryptedIdentity;
        setRouters(infoData.allowed_routers);
        setOsName(infoData.name);
      } catch { }
    })();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // for if we check router validity in future
  // const KEY_BAD_ROUTERS = "Routers from records are offline"

  const handleLogin = useCallback(
    async (e?: FormEvent) => {
      e?.preventDefault();
      e?.stopPropagation();

      try {
        if (reset) {
          if (!provider) {
            setKeyErrs(["Please connect your wallet and try again"]);
            setRestartFlow(true);
            return openConnect();
          }

          setLoading("Checking password...");

          let hashed_password = utils.sha256(utils.toUtf8Bytes(pw));

          // Replace this with network key generation
          const response = await fetch("/vet-keyfile", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ password_hash: hashed_password, keyfile: "" }),
          });

          if (response.status > 399) {
            throw new Error("Incorrect password");
          }

          // Generate keys on server that are stored temporarily
          const {
            networking_key,
            routing: {
              Both: {
                ip: ip_address,
                ports: {
                  ws: port
                },
                routers: allowed_routers
              }
            },
          } = (await fetch("/generate-networking-info", {
            method: "POST",
          }).then((res) => res.json())) as NetworkingInfo;

          setLoading("Please confirm the transaction in your wallet");

          const ipAddress = ipToNumber(ip_address);

          const data: BytesLike[] = [
            direct
              ? (
                await kns.populateTransaction.setAllIp(
                  namehash(knsName),
                  ipAddress,
                  port,
                  0,
                  0,
                  0
                )
              ).data!
              : (
                await kns.populateTransaction.setRouters(
                  namehash(knsName),
                  allowed_routers.map((x) => namehash(x))
                )
              ).data!,
            (
              await kns.populateTransaction.setKey(
                namehash(knsName),
                networking_key
              )
            ).data!,
          ];

          setLoading("Please confirm the transaction");

          const tx = await kns.multicall(data);

          setLoading("Resetting Networking Information...");

          await tx.wait();
        }

        setLoading("Logging in...");
        let hashed_password = utils.sha256(utils.toUtf8Bytes(pw));

        // Login or confirm new keys
        const result = await fetch(
          reset ? "/api/confirm-change-network-keys" : "login",
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: reset
              ? JSON.stringify({ password_hash: hashed_password, direct })
              : JSON.stringify({ password_hash: hashed_password }),
          }
        );

        if (result.status > 399) {
          throw new Error(await result.text());
        }

        if (reset) {
          const base64String = await result.json();
          downloadKeyfile(knsName, base64String);
        }

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
      } catch (err: any) {
        const errorString = String(err);
        if (errorString.includes("Object")) {
          setKeyErrs([
            "There was an error with the transaction, or it was cancelled.",
          ]);
        } else {
          setKeyErrs([errorString]);
        }
        setLoading("");
      }
    },
    [pw, appSizeOnLoad, reset, direct, knsName, provider, openConnect, kns]
  );

  const isDirect = Boolean(routers?.length === 0);

  const isMobile = isMobileCheck()

  return (
    <>
      <KinodeHeader
        header={<KinodeTitle prefix='Login to' showLogo />}
        openConnect={openConnect}
        closeConnect={closeConnect}
        hideConnect={!showReset}
        nodeChainId={nodeChainId}
      />
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

          <button type="submit" className="w-full mb-2"> {reset ? "Reset & " : ""} Login </button>

          <div className="flex flex-col w-full self-stretch place-content-center place-items-center">
            <button
              className="clear self-stretch mb-1"
              onClick={() => {
                setShowReset(!showReset);
                setReset(!showReset);
              }}
            >
              {showReset ? 'Cancel' : 'Reset Networking Info'}
            </button>
            <button
              className="clear self-stretch"
              onClick={() => {
                navigate('/reset-node')
              }}
            >
              Reset Node & Password
            </button>
            {showReset && (
              <div
                className="flex flex-col w-full gap-2 mt-4"
              >
                <div className="flex w-full place-items-center">
                  <div className="relative flex">
                    <input
                      type="checkbox"
                      id="reset"
                      name="reset"
                      checked={reset}
                      onChange={(e) => setReset(e.target.checked)}
                      autoFocus
                      className="mr-2"
                    />
                    {reset && (
                      <span
                        onClick={() => setReset(false)}
                        className="checkmark"
                      >
                        &#10003;
                      </span>
                    )}
                  </div>
                  <label htmlFor="reset" className="direct-node-message">
                    Reset networking keys and publish on-chain
                  </label>
                  <Tooltip text={`This will update your networking keys and publish the new info on-chain`} />
                </div>
                <DirectCheckbox {...{ direct, setDirect }} />
              </div>
            )}
          </div>
        </form >
      )
      }
    </>
  );
}

export default Login;
