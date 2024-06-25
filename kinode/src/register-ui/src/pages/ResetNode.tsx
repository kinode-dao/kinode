import {
    FormEvent,
    useCallback,
    useEffect,
    useState,
} from "react";
import { Link, useNavigate } from "react-router-dom";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";
import { generateNetworkingKeys, getNetworkName } from "../utils/chain";
import DirectCheckbox from "../components/DirectCheckbox";
import { namehash } from "@ethersproject/hash";

import { useAccount } from "wagmi";

interface ResetProps extends PageProps { }

function ResetNode({
    direct,
    setDirect,
    setReset,
    knsName,
    openConnect,
    setNetworkingKey,
    setIpAddress,
    setWsPort,
    setTcpPort,
    setRouters,
    nodeChainId,
}: ResetProps) {
    const { address } = useAccount();
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


            setLoading("Please confirm the transaction in your wallet");
            try {
                // TODO
                const data = await generateNetworkingKeys({
                    direct,
                    kns: "kns here",
                    nodeChainId,
                    chainName,
                    nameToSet: namehash(knsName),
                    setNetworkingKey,
                    setIpAddress,
                    setWsPort,
                    setTcpPort,
                    setRouters,
                });

                // const tx = await kns.multicall(data);

                setLoading("Resetting Networking Information...");

                // await tx.wait();

                setReset(true);
                setDirect(direct);
                navigate("/set-password");
            } catch {
                alert("An error occurred, please try again.");
            } finally {
                setLoading("");
            }
        },
        [
            knsName,
            setReset,
            setDirect,
            navigate,
            openConnect,
            direct,
            setNetworkingKey,
            setIpAddress,
            setWsPort,
            setTcpPort,
            setRouters,
            nodeChainId,
            chainName,
        ]
    );

    return (
        <>
            {Boolean(address) ? (
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
