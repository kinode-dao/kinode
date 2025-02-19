import { useState, useEffect, FormEvent, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";

import DirectCheckbox from "../components/DirectCheckbox";

import { useAccount, useWaitForTransactionReceipt, useSendTransaction } from "wagmi";
import { useConnectModal, useAddRecentTransaction } from "@rainbow-me/rainbowkit"
import { tbaMintAbi, generateNetworkingKeys, KINO_ACCOUNT_IMPL } from "../abis";
import { encodePacked, encodeFunctionData, stringToHex } from "viem";

interface MintCustomNameProps extends PageProps { }

function MintCustom({
    direct,
    setDirect,
    hnsName,
    setHnsName,
    setNetworkingKey,
    setIpAddress,
    setWsPort,
    setTcpPort,
    setRouters,
}: MintCustomNameProps) {
    let { address } = useAccount();
    let navigate = useNavigate();
    let { openConnectModal } = useConnectModal();

    const { data: hash, sendTransaction, isPending, isError, error } = useSendTransaction({
        mutation: {
            onSuccess: (data) => {
                addRecentTransaction({ hash: data, description: `Mint ${hnsName}` });
            }
        }
    });
    const { isLoading: isConfirming, isSuccess: isConfirmed } =
        useWaitForTransactionReceipt({
            hash,
        });
    const addRecentTransaction = useAddRecentTransaction();

    const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false)

    useEffect(() => {
        document.title = "Mint"
    }, [])

    useEffect(() => setTriggerNameCheck(!triggerNameCheck), [address])

    useEffect(() => {
        if (!address) {
            openConnectModal?.();
        }
    }, [address, openConnectModal]);

    let handleMint = useCallback(async (e: FormEvent) => {
        e.preventDefault()
        e.stopPropagation()

        const formData = new FormData(e.target as HTMLFormElement)

        if (!address) {
            openConnectModal?.()
            return
        }

        const initCall = await generateNetworkingKeys({
            direct,
            our_address: address,
            label: hnsName,
            setNetworkingKey,
            setIpAddress,
            setWsPort,
            setTcpPort,
            setRouters,
            reset: false,
        });

        setHnsName(formData.get('full-hns-name') as string)

        const name = formData.get('name') as string

        console.log("full hns name", formData.get('full-hns-name'))
        console.log("name", name)

        const data = encodeFunctionData({
            abi: tbaMintAbi,
            functionName: 'mint',
            args: [
                address,
                encodePacked(["bytes"], [stringToHex(name)]),
                initCall,
                KINO_ACCOUNT_IMPL,
            ],
        })

        // use data to write to contract -- do NOT use writeContract
        // writeContract will NOT generate the correct selector for some reason
        // probably THEIR bug.. no abi works
        try {
            sendTransaction({
                to: formData.get('tba') as `0x${string}`,
                data: data,
                gas: 1000000n,
            })
        } catch (error) {
            console.error('Failed to send transaction:', error)
        }
    }, [direct, address, sendTransaction, setNetworkingKey, setIpAddress, setWsPort, setTcpPort, setRouters, openConnectModal])

    useEffect(() => {
        if (isConfirmed) {
            navigate("/set-password");
        }
    }, [isConfirmed, address, navigate]);

    return (
        <div className="container fade-in">
            <button onClick={() => history.back()} className="button secondary back">🔙</button>
            <div className="section">
                {
                    <form className="form" onSubmit={handleMint}>
                        {isPending || isConfirming ? (
                            <Loader msg={isConfirming ? 'Minting name...' : 'Please confirm the transaction in your wallet'} />
                        ) : (
                            <>
                                <p className="form-label">
                                    Register a name on a different top-level zone -- this will likely fail if that zone's requirements are not met
                                </p>
                                <input type="text" name="name" placeholder="Enter hypermap name" />
                                <input type="text" name="full-hns-name" placeholder="Enter full HNS name" />
                                <input type="text" name="tba" placeholder="Enter TBA to mint under" />
                                <DirectCheckbox {...{ direct, setDirect }} />
                                <div className="button-group">
                                    <button type="submit" className="button">
                                        Mint custom name
                                    </button>
                                </div>
                            </>
                        )}
                        {isError && (
                            <p className="error-message">
                                Error: {error?.message || 'There was an error minting your name, please try again.'}
                            </p>
                        )}
                    </form>
                }
            </div>
        </div>
    );
}

export default MintCustom;