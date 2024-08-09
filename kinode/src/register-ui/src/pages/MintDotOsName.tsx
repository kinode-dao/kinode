import { useState, useEffect, FormEvent, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";

import { useAccount, useWaitForTransactionReceipt, useSendTransaction } from "wagmi";
import { useConnectModal, useAddRecentTransaction } from "@rainbow-me/rainbowkit"
import { dotOsAbi, generateNetworkingKeys, KINO_ACCOUNT_IMPL, DOTOS } from "../abis";
import { encodePacked, parseAbi, encodeFunctionData, stringToHex } from "viem";

interface RegisterOsNameProps extends PageProps { }

function MintDotOsName({
  direct,
  knsName,
  setNetworkingKey,
  setIpAddress,
  setWsPort,
  setTcpPort,
  setRouters,
  commitSecret,
}: RegisterOsNameProps) {
  let { address } = useAccount();
  let navigate = useNavigate();
  let { openConnectModal } = useConnectModal();

  const { data: hash, sendTransaction, isPending, isError, error } = useSendTransaction({
    mutation: {
      onSuccess: (data) => {
        addRecentTransaction({ hash: data, description: `Mint ${knsName}` });
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

  let handleMint = useCallback(async (e: FormEvent) => {
    e.preventDefault()
    e.stopPropagation()

    if (!address) {
      openConnectModal?.()
      return
    }

    const initCall = await generateNetworkingKeys({
      direct,
      our_address: address,
      label: knsName,
      setNetworkingKey,
      setIpAddress,
      setWsPort,
      setTcpPort,
      setRouters,
      reset: false,
    });

    console.log("minting name: ", knsName)

    // strip .os suffix
    const name = knsName.replace(/\.os$/, '');

    const abi = parseAbi([
      'function mint(address,bytes,bytes,bytes,address,bytes32)',
    ])

    const data = encodeFunctionData({
      abi,
      functionName: 'mint',
      args: [
        address,
        encodePacked(["bytes"], [stringToHex(name)]),
        initCall,
        "0x",
        KINO_ACCOUNT_IMPL,
        commitSecret
      ],
    })

    console.log("data: ", data)

    // use data to write to contract -- do NOT use writeContract
    // writeContract will NOT generate the correct selector for some reason
    // probably THEIR bug.. no abi works
    try {
      sendTransaction({
        to: DOTOS,
        data: data,
        gas: 1000000n,
      })
      console.log('Transaction sent?')
      // You might want to add some state management here to track the transaction
    } catch (error) {
      console.error('Failed to send transaction:', error)
      // Handle the error appropriately, maybe set an error state
    }
  }, [direct, address, sendTransaction, setNetworkingKey, setIpAddress, setWsPort, setTcpPort, setRouters, openConnectModal])

  useEffect(() => {
    if (isConfirmed) {
      navigate("/set-password");
    }
  }, [isConfirmed, address, navigate]);

  return (
    <div className="container fade-in">
      <div className="section">
        {Boolean(address) && (
          <form className="form" onSubmit={handleMint}>
            {isPending || isConfirming ? (
              <Loader msg={isConfirming ? 'Minting .os name...' : 'Please confirm the transaction in your wallet'} />
            ) : (
              <>
                <div className="button-group">
                  <button type="submit" className="button">
                    Mint pre-committed .os name
                  </button>
                </div>
              </>
            )}
            {isError && (
              <p className="error-message">
                Error: {error?.message || 'There was an error minting your dot-os-name, please try again.'}
              </p>
            )}
          </form>
        )}
      </div>
    </div>
  );
}

export default MintDotOsName;