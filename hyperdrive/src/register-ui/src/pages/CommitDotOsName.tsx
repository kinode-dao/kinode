import { useState, useEffect, FormEvent, useCallback } from "react";
import { Link, useNavigate } from "react-router-dom";
import { toAscii } from "idna-uts46-hx";
import EnterHnsName from "../components/EnterHnsName";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";

import DirectCheckbox from "../components/DirectCheckbox";
import { Tooltip } from "../components/Tooltip";

import { useAccount, useWaitForTransactionReceipt, useWriteContract } from "wagmi";
import { useConnectModal, useAddRecentTransaction } from "@rainbow-me/rainbowkit"
import { dotOsAbi, DOTOS } from "../abis";
import { stringToHex, encodeAbiParameters, parseAbiParameters, keccak256 } from "viem";

interface RegisterOsNameProps extends PageProps { }

function CommitDotOsName({
    direct,
    setDirect,
    setHnsName,
    setNetworkingKey,
    setIpAddress,
    setWsPort,
    setTcpPort,
    setRouters,
}: RegisterOsNameProps) {
    let { address } = useAccount();
    let navigate = useNavigate();
    let { openConnectModal } = useConnectModal();

    const { data: hash, writeContract, isPending, isError, error } = useWriteContract({
        mutation: {
            onSuccess: (data) => {
                addRecentTransaction({ hash: data, description: `Pre-commit to .os ID: ${name}.os` });
            }
        }
    });
    const { isLoading: isConfirming, isSuccess: txConfirmed } =
        useWaitForTransactionReceipt({
            hash,
        });
    const addRecentTransaction = useAddRecentTransaction();

    const [name, setName] = useState('')
    const [nameValidities, setNameValidities] = useState<string[]>([])
    const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false)
    const [isConfirmed, setIsConfirmed] = useState(false)

    useEffect(() => {
        document.title = "Register"
    }, [])

    useEffect(() => setTriggerNameCheck(!triggerNameCheck), [address])

    const enterOsNameProps = { address, name, setName, fixedTlz: ".os", nameValidities, setNameValidities, triggerNameCheck }

    useEffect(() => {
        if (!address) {
            openConnectModal?.();
        }
    }, [address, openConnectModal]);

    let handleCommit = useCallback(async (e: FormEvent) => {
        e.preventDefault()
        e.stopPropagation()
        if (!address) {
            openConnectModal?.()
            return
        }
        setName(toAscii(name));
        console.log("committing to .os name: ", name)
        const commit = keccak256(
            encodeAbiParameters(
                parseAbiParameters('bytes memory, address'),
                [stringToHex(name), address]
            )
        )
        writeContract({
            abi: dotOsAbi,
            address: DOTOS,
            functionName: 'commit',
            args: [commit],
            gas: 1000000n,
        })

    }, [name, direct, address, writeContract, setNetworkingKey, setIpAddress, setWsPort, setTcpPort, setRouters, openConnectModal])

    useEffect(() => {
        if (txConfirmed) {
            console.log("confirmed commit to .os name: ", name)
            console.log("waiting 16 seconds to make commit valid...")
            setTimeout(() => {
                setIsConfirmed(true);
                setHnsName(`${name}.os`);
                navigate("/mint-os-name");
            }, 16000)
        }
    }, [txConfirmed, address, name, setHnsName, navigate]);

    return (
        <div className="container fade-in">
            <button onClick={() => history.back()} className="button secondary back">🔙</button>
            <div className="section">
                {
                    <form className="form" onSubmit={handleCommit}>
                        {isPending || isConfirming || (txConfirmed && !isConfirmed) ? (
                            <Loader msg={
                                isConfirming ? 'Pre-committing to chosen name...' :
                                    (txConfirmed && !isConfirmed) ? 'Waiting 15s for commit to become valid...' :
                                        'Please confirm the transaction in your wallet'
                            } />
                        ) : (
                            <>
                                <h3 className="form-label">
                                    <Tooltip text="Nodes need an onchain node identity in order to communicate with other nodes in the network.">
                                        Choose a name for your node
                                    </Tooltip>
                                </h3>
                                <EnterHnsName {...enterOsNameProps} />
                                <details>
                                    <summary>Advanced Options</summary>
                                    <DirectCheckbox {...{ direct, setDirect }} />
                                </details>
                                <div className="button-group">
                                    <button
                                        disabled={nameValidities.length !== 0 || isPending || isConfirming}
                                        type="submit"
                                        className="button"
                                    >
                                        Register name
                                    </button>
                                    <Link to="/reset" className="button secondary">
                                        Already have a node?
                                    </Link>
                                </div>
                            </>
                        )}
                        {isError && (
                            <p className="error-message">
                                Error: {error?.message || 'There was an error registering your name, please try again.'}
                            </p>
                        )}
                    </form>
                }
            </div>
        </div>
    );
}

export default CommitDotOsName;
