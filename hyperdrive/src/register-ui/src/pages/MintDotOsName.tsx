import { useState, useEffect, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";

import { useAccount, useWaitForTransactionReceipt, useSendTransaction } from "wagmi";
import { useConnectModal, useAddRecentTransaction } from "@rainbow-me/rainbowkit"
import { generateNetworkingKeys, KINO_ACCOUNT_IMPL, DOTOS, tbaMintAbi } from "../abis";
import { encodePacked, encodeFunctionData, stringToHex } from "viem";

interface RegisterOsNameProps extends PageProps { }

function MintDotOsName({
  direct,
  hnsName,
  setNetworkingKey,
  setIpAddress,
  setWsPort,
  setTcpPort,
  setRouters,
}: RegisterOsNameProps) {
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

  const [hasMinted, setHasMinted] = useState(false);

  useEffect(() => {
    document.title = "Mint"
  }, [])

  useEffect(() => {
    if (!address) {
      openConnectModal?.();
    }
  }, [address, openConnectModal]);

  const handleMint = useCallback(async () => {
    if (!address) {
      openConnectModal?.()
      return
    }
    if (hasMinted) {
      return
    }

    setHasMinted(true);

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

    // strip .os suffix
    const name = hnsName.replace(/\.os$/, '');

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
        to: DOTOS,
        data: data,
        gas: 1000000n,
      })
    } catch (error) {
      console.error('Failed to send transaction:', error)
      setHasMinted(false);
    }
  }, [direct, address, sendTransaction, setNetworkingKey, setIpAddress, setWsPort, setTcpPort, setRouters, openConnectModal, hnsName, hasMinted])

  useEffect(() => {
    if (address && !isPending && !isConfirming) {
      handleMint();
    }
  }, [address, handleMint, isPending, isConfirming]);

  useEffect(() => {
    if (isConfirmed) {
      navigate("/set-password");
    }
  }, [isConfirmed, address, navigate]);

  return (
    <div className="container fade-in">
      <div className="section">
        <div className="form">
          {isPending || isConfirming ? (
            <Loader msg={isConfirming ? 'Minting name...' : 'Please confirm the transaction in your wallet'} />
          ) : (
            <Loader msg="Preparing to mint..." />
          )}
          {isError && (
            <p className="error-message">
              Error: {error?.message || 'There was an error minting your name, please try again.'}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

export default MintDotOsName;