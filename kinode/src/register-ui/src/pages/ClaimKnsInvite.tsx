import React, { useState, useEffect, FormEvent } from "react";
import { hooks } from "../connectors/metamask";
import { Link, useNavigate } from "react-router-dom";
import EnterKnsName from "../components/EnterKnsName";
import Loader from "../components/Loader";
import KinodeHeader from "../components/KnsHeader";
import { NetworkingInfo, PageProps } from "../lib/types";
import { ipToNumber } from "../utils/ipToNumber";
import DirectCheckbox from "../components/DirectCheckbox";
import { ReactComponent as NameLogo } from "../assets/kinode.svg"
import { KinodeTitle } from "../components/KinodeTitle";
import { Tooltip } from "../components/Tooltip";

global.Buffer = global.Buffer || require("buffer").Buffer;

const { useAccounts, useProvider } = hooks;

interface ClaimOsNameProps extends PageProps { }

function ClaimOsInvite({
  direct,
  setDirect,
  setOsName,
  dotOs,
  openConnect,
  setNetworkingKey,
  setIpAddress,
  setPort,
  setRouters,
  closeConnect,
  nodeChainId,
}: ClaimOsNameProps) {
  const accounts = useAccounts();
  const provider = useProvider();
  const navigate = useNavigate();

  const [isLoading, setIsLoading] = useState(false);
  const [loaderMsg, setLoaderMsg] = useState("");
  const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false);
  const [invite, setInvite] = useState("");
  const [inviteValidity, setInviteValidity] = useState("");
  const [name, setName] = useState("");
  const [nameValidities, setNameValidities] = useState<string[]>([]);

  useEffect(() => {
    document.title = "Claim Invite";
  }, []);

  useEffect(() => setTriggerNameCheck(!triggerNameCheck), [provider]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    (async () => {
      if (invite !== "") {
        const url = process.env.REACT_APP_INVITE_GET + invite;

        const response = await fetch(url, { method: "GET" });

        if (response!.status === 200) {
          setInviteValidity("");
        } else {
          setInviteValidity(await response.text());
        }
      }
    })();
  }, [invite]);

  let handleRegister = async (e: FormEvent) => {
    e.preventDefault();
    e.stopPropagation();

    if (!provider) return openConnect();

    const {
      networking_key,
      ws_routing: [ip_address, port],
      allowed_routers,
    } = (await fetch("/generate-networking-info", { method: "POST" }).then(
      (res) => res.json()
    )) as NetworkingInfo;

    const ipAddress = ipToNumber(ip_address);

    setNetworkingKey(networking_key);
    setIpAddress(ipAddress);
    setPort(port);
    setRouters(allowed_routers);

    if (nameValidities.length !== 0 || inviteValidity !== "") return;
    if (!name || !invite) {
      window.alert("Please enter a name and invite code");
      return false;
    }

    let response;

    setLoaderMsg("...Building EIP-4337 User Operation");
    setIsLoading(true);

    console.log("BUILDING", networking_key, ipAddress, port, allowed_routers);

    try {
      response = await fetch(process.env.REACT_APP_BUILD_USER_OP_POST!, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          name: name + ".os",
          address: accounts![0],
          networkingKey: networking_key,
          wsIp: ipAddress,
          wsPort: port,
          routers: allowed_routers,
          direct: direct,
        }),
      });
    } catch (e) {
      setLoaderMsg("");
      setIsLoading(false);

      alert(e);

      console.error("error from fetching userOp:", e);

      return;
    }

    setLoaderMsg("...Signing EIP-4337 User Operation");

    const data = await response.json();

    const uint8Array = new Uint8Array(Object.values(data.message));

    const signer = await provider?.getSigner();
    const signature = await signer?.signMessage(uint8Array);

    data.userOperation.signature = signature;

    try {
      response = await fetch(process.env.REACT_APP_BROADCAST_USER_OP_POST!, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          userOp: data.userOperation,
          code: invite,
          name: name + ".os",
          eoa: accounts![0],
        }),
      });
    } catch (e) {
      alert(e);
      console.error("error from broadcasting userOp:", e);
      return;
    } finally {
      setLoaderMsg("");
      setIsLoading(false);
    }

    setOsName(`${name}.os`);

    navigate("/set-password");
  };

  const enterOsNameProps = {
    name,
    setName,
    nameValidities,
    setNameValidities,
    dotOs,
    triggerNameCheck,
  };

  return (
    <>
      <KinodeHeader
        header={<KinodeTitle prefix="Claim Invite" />}
        openConnect={openConnect}
        closeConnect={closeConnect}
        nodeChainId={nodeChainId}
      />
      {Boolean(provider) && (
        <form id="signup-form" className="flex flex-col" onSubmit={handleRegister}>
          {isLoading ? (
            <Loader msg={loaderMsg} />
          ) : (
            <>
              <div className="flex c mb-2">
                <h5>Set up your Kinode with a .os name</h5>
                <Tooltip text={`Kinodes use a .os name in order to identify themselves to other nodes in the network.`} />
              </div>

              <div className="flex flex-col mb-2">
                <input
                  value={invite}
                  onChange={(e) => setInvite(e.target.value)}
                  type="text"
                  required
                  name="nec-invite"
                  placeholder="invite code"
                  className="self-stretch"
                />
                {inviteValidity !== "" && (
                  <div className="invite-validity">{inviteValidity}</div>
                )}
              </div>

              <h3 className="mb-2">
                <EnterKnsName {...enterOsNameProps} />
              </h3>

              <DirectCheckbox {...{ direct, setDirect }} />

              <button
                disabled={nameValidities.length !== 0 || inviteValidity !== ""}
                type="submit"
                className="self-stretch mt-2"
              >
                Register .os name
              </button>

              <Link to="/reset" className="button clear">
                already have a .os?
              </Link>
            </>
          )}
        </form >
      )
      }
    </>
  );
}

export default ClaimOsInvite;
