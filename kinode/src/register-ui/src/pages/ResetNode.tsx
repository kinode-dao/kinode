import {
    FormEvent,
    useCallback,
    useEffect,
    useState,
} from "react";
import { useNavigate } from "react-router-dom";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";
import { MULTICALL, generateNetworkingKeys, mechAbi } from "../abis";
import DirectCheckbox from "../components/DirectCheckbox";

import { useAccount, useWaitForTransactionReceipt, useWriteContract } from "wagmi";
import { useConnectModal } from "@rainbow-me/rainbowkit";

function ResetNode({
    direct,
    setDirect,
    setReset,
    knsName,
    setNetworkingKey,
    setIpAddress,
    setWsPort,
    setTcpPort,
    setRouters,
}: PageProps) {
    const { address } = useAccount();
    const navigate = useNavigate();
    const { openConnectModal } = useConnectModal();

    const { data: hash, writeContract, isPending, isError, error } = useWriteContract();
    const { isLoading: isConfirming, isSuccess: isConfirmed } =
        useWaitForTransactionReceipt({
            hash,
        })

    const [tba, setTba] = useState<string>("");

    useEffect(() => {
        document.title = "Reset";
        // Here you would fetch the TBA (Token Bound Account) for the given knsName
        // This is a placeholder and should be replaced with actual logic
        const fetchTba = async () => {
            // Placeholder: fetch TBA based on knsName
            // const fetchedTba = await fetchTbaForKnsName(knsName);
            // setTba(fetchedTba);
        };
        fetchTba();
    }, [knsName]);

    const handleResetRecords = useCallback(
        async (e: FormEvent) => {
            e.preventDefault();
            e.stopPropagation();

            if (!address) {
                openConnectModal?.();
                return;
            }

            try {
                const data = await generateNetworkingKeys({
                    direct,
                    label: knsName.slice(0, -3), // Remove '.os' from the end
                    our_address: address,
                    setNetworkingKey,
                    setIpAddress,
                    setWsPort,
                    setTcpPort,
                    setRouters,
                    reset: true,
                });

                writeContract({
                    address: tba as `0x${string}`,
                    abi: mechAbi,
                    functionName: "execute",
                    args: [
                        MULTICALL,
                        BigInt(0),
                        data,
                        1
                    ],
                    gas: 1000000n,
                });
            } catch (error) {
                console.error("An error occurred:", error);
            }
        },
        [address, direct, tba, knsName, setNetworkingKey, setIpAddress, setWsPort, setTcpPort, setRouters, writeContract, openConnectModal]
    );

    useEffect(() => {
        if (isConfirmed) {
            setReset(true);
            setDirect(direct);
            navigate("/set-password");
        }
    }, [isConfirmed, setReset, setDirect, direct, navigate]);

    return (
        <>
            {Boolean(address) ? (
                <form id="signup-form" className="flex flex-col" onSubmit={handleResetRecords}>
                    {isPending || isConfirming ? (
                        <Loader msg={isConfirming ? "Resetting Networking Information..." : "Please confirm the transaction in your wallet"} />
                    ) : (
                        <>
                            <h3 className="text-center mb-4">
                                Reset Node: {knsName}
                            </h3>

                            <DirectCheckbox {...{ direct, setDirect }} />

                            <button
                                type="submit"
                                className="mt-4 text-xl"
                                disabled={isPending || isConfirming}
                            >
                                Reset Node
                            </button>
                        </>
                    )}
                    {isError && (
                        <p className="text-red-500 mt-2">
                            Error: {error?.message || "An error occurred, please try again."}
                        </p>
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