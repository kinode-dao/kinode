import { useState, useEffect, FormEvent, useCallback } from "react";
import { Link, useNavigate } from "react-router-dom";
import { toAscii } from "idna-uts46-hx";
import EnterKnsName from "../components/EnterKnsName";
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
    setKnsName,
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
    const { isLoading: isConfirming, isSuccess: isConfirmed } =
        useWaitForTransactionReceipt({
            hash,
        });
    const addRecentTransaction = useAddRecentTransaction();

    const [name, setName] = useState('')
    const [nameValidities, setNameValidities] = useState<string[]>([])
    const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false)

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
        const commitSecret = keccak256(stringToHex(name))
        const commit = keccak256(
            encodeAbiParameters(
                parseAbiParameters('bytes32, bytes32'),
                [keccak256(stringToHex(name)), commitSecret]
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
        if (isConfirmed) {
            setKnsName(`${name}.os`);
            navigate("/mint-os-name");
        }
    }, [isConfirmed, address, name, setKnsName, navigate]);

    return (
        <div className="container fade-in">
            <button onClick={() => history.back()} className="button secondary">🔙</button>
            <div className="section">
                {
                    <form className="form" onSubmit={handleCommit}>
                        {isPending || isConfirming ? (
                            <Loader msg={isConfirming ? 'Pre-committing to chosen name...' : 'Please confirm the transaction in your wallet'} />
                        ) : (
                            <>
                                <h3 className="form-label">
                                    <Tooltip text="Kinodes need an onchain node identity in order to communicate with other nodes in the network.">
                                        Choose a name for your node
                                    </Tooltip>
                                </h3>
                                <EnterKnsName {...enterOsNameProps} />
                                <DirectCheckbox {...{ direct, setDirect }} />
                                <div className="button-group">
                                    <button
                                        disabled={nameValidities.length !== 0 || isPending || isConfirming}
                                        type="submit"
                                        className="button"
                                    >
                                        Register name
                                    </button>
                                    <p>This will confirm availability of the name and reserve it, then on the next screen you will be prompted to mint.</p>
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
