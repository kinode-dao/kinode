import {
    FormEvent,
    useCallback,
    useEffect,
    useState,
} from "react";
import { hooks } from "../connectors/metamask";
import { Link, useNavigate } from "react-router-dom";
import { namehash } from "ethers/lib/utils";
import Loader from "../components/Loader";
import KinodeHeader from "../components/KnsHeader";
import { NetworkingInfo, PageProps } from "../lib/types";
import { ipToNumber } from "../utils/ipToNumber";
import { getNetworkName, setChain } from "../utils/chain";
import DirectCheckbox from "../components/DirectCheckbox";

const { useProvider } = hooks;

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
            <KinodeHeader header={<h1 className="flex c mb-8">
                Reset Kinode Name
            </h1>}
                openConnect={openConnect}
                closeConnect={closeConnect}
                nodeChainId={nodeChainId}
            />
            {Boolean(provider) ? (
                <form
                    id="signup-form"
                    className="flex flex-col"
                    onSubmit={handleResetRecords}
                >
                    {loading ? (
                        <Loader msg={loading} />
                    ) : (
                        <>
                            <DirectCheckbox {...{ direct, setDirect }} />

                            <button type="submit" className="self-stretch mt-2 text-2xl">
                                Reset {knsName}
                            </button>
                            <Link to="/" className="button alt mt-2">
                                Back
                            </Link>
                        </>
                    )}
                </form>
            ) : (
                <div>
                    Please connect a wallet to continue.
                </div>
            )}
        </>
    );
}

export default ResetNode;
