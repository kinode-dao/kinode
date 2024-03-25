import React, {
  FormEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { hooks } from "../connectors/metamask";
import { useNavigate } from "react-router-dom";
import { namehash } from "ethers/lib/utils";
import { toAscii } from "idna-uts46-hx";
import { hash } from "eth-ens-namehash";
import isValidDomain from "is-valid-domain";
import Loader from "../components/Loader";
import OsHeader from "../components/KnsHeader";
import { NetworkingInfo, PageProps } from "../lib/types";
import { ipToNumber } from "../utils/ipToNumber";
import { getNetworkName, setChain } from "../utils/chain";
import { ReactComponent as NameLogo } from "../assets/kinode.svg"

const NAME_INVALID_PUNY = "Unsupported punycode character";
const NAME_NOT_OWNER = "Name does not belong to this wallet";
const NAME_NOT_REGISTERED = "Name is not registered";
const NAME_URL =
  "Name must be a valid URL without subdomains (A-Z, a-z, 0-9, and punycode)";

const { useAccounts, useProvider } = hooks;

interface ResetProps extends PageProps { }

function Reset({
  direct,
  setDirect,
  setReset,
  knsName,
  setOsName,
  dotOs,
  kns,
  openConnect,
  closeConnect,
  setNetworkingKey,
  setIpAddress,
  setPort,
  setRouters,
  nodeChainId,
}: ResetProps) {
  const accounts = useAccounts();
  const provider = useProvider();
  const navigate = useNavigate();

  const chainName = getNetworkName(nodeChainId);
  const [name, setName] = useState<string>(knsName.slice(0, -3));
  const [nameVets, setNameVets] = useState<string[]>([]);
  const [loading, setLoading] = useState<string>("");

  const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false);

  useEffect(() => {
    document.title = "Reset";
  }, []);

  // so inputs will validate once wallet is connected
  useEffect(() => setTriggerNameCheck(!triggerNameCheck), [provider]); // eslint-disable-line react-hooks/exhaustive-deps

  const nameDebouncer = useRef<NodeJS.Timeout | null>(null);
  useEffect(() => {
    if (nameDebouncer.current) clearTimeout(nameDebouncer.current);

    nameDebouncer.current = setTimeout(async () => {
      setNameVets([]);

      if (!provider) return;

      if (name === "") return;

      let index: number;
      let vets = [...nameVets];

      let normalized: string;
      index = vets.indexOf(NAME_INVALID_PUNY);
      try {
        normalized = toAscii(name + ".os");
        if (index !== -1) vets.splice(index, 1);
      } catch (e) {
        if (index === -1) vets.push(NAME_INVALID_PUNY);
      }

      // only check if name is valid punycode
      if (normalized! !== undefined) {
        index = vets.indexOf(NAME_URL);
        if (name !== "" && !isValidDomain(normalized)) {
          if (index === -1) vets.push(NAME_URL);
        } else if (index !== -1) vets.splice(index, 1);

        try {
          const owner = await dotOs.ownerOf(hash(normalized));

          index = vets.indexOf(NAME_NOT_OWNER);
          if (owner === accounts![0] && index !== -1) vets.splice(index, 1);
          else if (index === -1 && owner !== accounts![0])
            vets.push(NAME_NOT_OWNER);

          index = vets.indexOf(NAME_NOT_REGISTERED);
          if (index !== -1) vets.splice(index, 1);
        } catch (e) {
          index = vets.indexOf(NAME_NOT_REGISTERED);
          if (index === -1) vets.push(NAME_NOT_REGISTERED);
        }

        if (nameVets.length === 0) setOsName(normalized);
      }

      setNameVets(vets);
    }, 500);
  }, [name, triggerNameCheck]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleResetRecords = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (!provider) return openConnect();

      try {
        setLoading("Please confirm the transaction in your wallet");

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

        const data = [
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

        try {
          await setChain(nodeChainId);
        } catch (error) {
          window.alert(
            `You must connect to the ${chainName} network to continue. Please connect and try again.`
          );
          throw new Error(`${chainName} not set`);
        }

        const tx = await kns.multicall(data);

        setLoading("Resetting Networking Information...");

        await tx.wait();

        setReset(true);
        setLoading("");
        setDirect(direct);
        navigate("/set-password");
      } catch {
        setLoading("");
        alert("An error occurred, please try again.");
      }
    },
    [
      provider,
      knsName,
      setReset,
      setDirect,
      navigate,
      openConnect,
      kns,
      direct,
      setNetworkingKey,
      setIpAddress,
      setPort,
      setRouters,
      nodeChainId,
      chainName,
    ]
  );

  return (
    <>
      <OsHeader header={<h3 className="row" style={{ justifyContent: "center", alignItems: "center" }}>
        Reset
        <NameLogo style={{ height: 28, width: "auto", margin: "0 16px -3px" }} />
        Name
      </h3>}
        openConnect={openConnect}
        closeConnect={closeConnect}
        nodeChainId={nodeChainId}
      />
      {Boolean(provider) && (
        <form id="signup-form" className="col" onSubmit={handleResetRecords}>
          {loading ? (
            <Loader msg={loading} />
          ) : (
            <>
              <div className="col" style={{ width: "100%" }}>
                <h5 className="login-row row" style={{ marginBottom: 8 }}>
                  Enter .os Name
                  <div className="tooltip-container">
                    <div className="tooltip-button">&#8505;</div>
                    <div className="tooltip-content" style={{ fontSize: 16 }}>
                      Kinodes use a .os name in order to identify themselves to
                      other nodes in the network
                    </div>
                  </div>
                </h5>
                <div
                  style={{
                    display: "flex",
                    alignItems: "center",
                    width: "100%",
                    marginBottom: "0.5em",
                  }}
                >
                  <input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    type="text"
                    required
                    name="dot-os-name"
                    placeholder="e.g. myname"
                    style={{ width: "100%", marginRight: 8 }}
                  />
                  .os
                </div>
                {nameVets.map((x, i) => (
                  <span key={i} className="name-err">
                    {x}
                  </span>
                ))}
              </div>

              <div className="row">
                <div style={{ position: "relative" }}>
                  <input
                    type="checkbox"
                    id="direct"
                    name="direct"
                    checked={direct}
                    onChange={(e) => setDirect(e.target.checked)}
                    autoFocus
                  />
                  {direct && (
                    <span
                      onClick={() => setDirect(false)}
                      className="checkmark"
                    >
                      &#10003;
                    </span>
                  )}
                </div>
                <label htmlFor="direct" className="direct-node-message">
                  Register as a direct node. If you are unsure leave unchecked.
                </label>
                <div className="tooltip-container">
                  <div className="tooltip-button">&#8505;</div>
                  <div className="tooltip-content">
                    A direct node publishes its own networking information
                    on-chain: IP, port, so on. An indirect node relies on the
                    service of routers, which are themselves direct nodes. Only
                    register a direct node if you know what you’re doing and
                    have a public, static IP address.
                  </div>
                </div>
              </div>

              <button type="submit"> Reset Node </button>
            </>
          )}
        </form>
      )}
    </>
  );
}

export default Reset;
