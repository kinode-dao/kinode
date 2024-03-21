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

const { useAccounts, useProvider } = hooks;

interface ResetProps extends PageProps { }

function ResetNode({
    direct,
    setDirect,
    setReset,
    knsName,
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
    const [loading, setLoading] = useState<string>("");


    useEffect(() => {
        document.title = "Reset";
    }, []);


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
                                    {knsName}
                                </h5>
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

export default ResetNode;
