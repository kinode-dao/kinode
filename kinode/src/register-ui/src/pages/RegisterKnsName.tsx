import { useState, useEffect, FormEvent, useCallback } from "react";
import { Link, useNavigate } from "react-router-dom";
import EnterKnsName from "../components/EnterKnsName";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";

import DirectCheckbox from "../components/DirectCheckbox";
import { Tooltip } from "../components/Tooltip";

import { useAccount, useWaitForTransactionReceipt, useWriteContract } from "wagmi";
import { useConnectModal } from "@rainbow-me/rainbowkit"
import { KINOMAP, kinomapAbi, generateNetworkingKeys, KINO_ACCOUNT_IMPL, DOTOS } from "../abis";
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
  nodeChainId,
}: RegisterOsNameProps) {
  let { address } = useAccount();
  let navigate = useNavigate();

  let { openConnectModal } = useConnectModal();

  const { data: hash, writeContract, isPending, isError, error } = useWriteContract();
  const { isLoading: isConfirming, isSuccess: isConfirmed } =
    useWaitForTransactionReceipt({
      hash,
    })

  const [name, setName] = useState('')
  const [nameValidities, setNameValidities] = useState<string[]>([])

  const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false)

  useEffect(() => {
    document.title = "Register"
  }, [])

  useEffect(() => setTriggerNameCheck(!triggerNameCheck), [address]) // eslint-disable-line react-hooks/exhaustive-deps

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
    <>
      {Boolean(address) && (
        <form
          id="signup-form"
          className="flex flex-col w-full max-w-[450px]"
          onSubmit={handleRegister}
        >
          {isPending || isConfirming ? (
            <Loader msg={isConfirming ? 'Registering KNS ID...' : 'Please confirm the transaction in your wallet'} />
          ) : (
            <>
              <h3 className="flex flex-col w-full place-items-center my-8">
                <label className="flex leading-6 place-items-center mt-2 cursor-pointer mb-2">
                  Choose a name for your Kinode
                  <Tooltip text={`Kinodes need an onchain node identity in order to communicate with other nodes in the network.`} />
                </label>
                <EnterKnsName {...enterOsNameProps} />
              </h3>
              <DirectCheckbox {...{ direct, setDirect }} />
              <button
                disabled={nameValidities.length !== 0 || isPending || isConfirming}
                type="submit"
                className="mt-2"
              >
                Register .os name
              </button>
              <Link to="/reset" className="flex self-stretch mt-2">
                <button className="clear grow">
                  already have a dot-os-name?
                </button>
              </Link>
            </>
          )}
          {isError && (
            <p className="text-red-500 mt-2">
              Error: {error?.message || 'There was an error registering your dot-os-name, please try again.'}
            </p>
          )}
        </form>
      )}
    </>
  )
}

export default RegisterKnsName;
