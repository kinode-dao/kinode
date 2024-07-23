import { useState, useEffect, FormEvent, useCallback } from "react";
import { Link, useNavigate } from "react-router-dom";
import EnterKnsName from "../components/EnterKnsName";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";

import DirectCheckbox from "../components/DirectCheckbox";
import { Tooltip } from "../components/Tooltip";

import { useAccount, useWaitForTransactionReceipt, useWriteContract } from "wagmi";
import { useConnectModal, useAddRecentTransaction } from "@rainbow-me/rainbowkit"
import { kinomapAbi, generateNetworkingKeys, KINO_ACCOUNT_IMPL, DOTOS } from "../abis";
import { encodePacked, stringToHex } from "viem";

interface RegisterOsNameProps extends PageProps { }

function RegisterKnsName({
  direct,
  setDirect,
  setOsName,
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
        addRecentTransaction({ hash: data, description: `Register KNS ID: ${name}.os` });
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

  const enterOsNameProps = { name, setName, nameValidities, setNameValidities, triggerNameCheck }

  let handleRegister = useCallback(async (e: FormEvent) => {
    e.preventDefault()
    e.stopPropagation()

    if (!address) {
      openConnectModal?.()
      return
    }

    const initCall = await generateNetworkingKeys({
      direct,
      our_address: address,
      label: name,
      setNetworkingKey,
      setIpAddress,
      setWsPort,
      setTcpPort,
      setRouters,
      reset: false,
    });

    writeContract({
      abi: kinomapAbi,
      address: DOTOS,
      functionName: 'mint',
      args: [
        address,
        encodePacked(["bytes"], [stringToHex(name)]),
        initCall,
        "0x",
        KINO_ACCOUNT_IMPL,
      ],
      gas: 1000000n,
    })
  }, [name, direct, address, writeContract, setNetworkingKey, setIpAddress, setWsPort, setTcpPort, setRouters, openConnectModal])

  useEffect(() => {
    if (isConfirmed) {
      setOsName(`${name}.os`);
      navigate("/set-password");
    }
  }, [isConfirmed, name, setOsName, navigate]);

  return (
    <div className="container fade-in">
      <div className="section">
        {Boolean(address) && (
          <form className="form" onSubmit={handleRegister}>
            {isPending || isConfirming ? (
              <Loader msg={isConfirming ? 'Registering KNS ID...' : 'Please confirm the transaction in your wallet'} />
            ) : (
              <>
                <h3 className="form-label">
                  <Tooltip text="Kinodes need an onchain node identity in order to communicate with other nodes in the network.">
                    Choose a name for your Kinode
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
                    Register .os name
                  </button>
                  <Link to="/reset" className="button secondary">
                    Already have a dot-os-name?
                  </Link>
                </div>
              </>
            )}
            {isError && (
              <p className="error-message">
                Error: {error?.message || 'There was an error registering your dot-os-name, please try again.'}
              </p>
            )}
          </form>
        )}
      </div>
    </div>
  );
}

export default RegisterKnsName;