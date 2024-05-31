import { useState, useEffect, FormEvent, useCallback } from "react";
import { hooks } from "../connectors/metamask";
import { Link, useNavigate } from "react-router-dom";
import { toDNSWireFormat } from "../utils/dnsWire";
import { utils } from 'ethers';
import EnterKnsName from "../components/EnterKnsName";
import Loader from "../components/Loader";
import KinodeHeader from "../components/KnsHeader";
import { PageProps } from "../lib/types";

import { generateNetworkingKeys, getNetworkName } from "../utils/chain";
import DirectCheckbox from "../components/DirectCheckbox";
import { Tooltip } from "../components/Tooltip";

const {
  useAccounts,
} = hooks;

interface RegisterOsNameProps extends PageProps { }

function RegisterKnsName({
  direct,
  setDirect,
  setOsName,
  dotOs,
  kns,
  openConnect,
  provider,
  closeConnect,
  setNetworkingKey,
  setIpAddress,
  setWsPort,
  setTcpPort,
  setRouters,
  nodeChainId,
}: RegisterOsNameProps) {
  let accounts = useAccounts();
  let navigate = useNavigate();
  const chainName = getNetworkName(nodeChainId);
  const [loading, setLoading] = useState('');

  const [name, setName] = useState('')
  const [nameValidities, setNameValidities] = useState<string[]>([])

  const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false)

  useEffect(() => {
    document.title = "Register"
  }, [])

  useEffect(() => setTriggerNameCheck(!triggerNameCheck), [provider]) // eslint-disable-line react-hooks/exhaustive-deps

  const enterOsNameProps = { name, setName, nameValidities, setNameValidities, dotOs, triggerNameCheck }

  let handleRegister = useCallback(async (e: FormEvent) => {
    e.preventDefault()
    e.stopPropagation()

    if (!provider || !kns) return openConnect()

    setLoading('Please confirm the transaction in your wallet');
    try {
      const nameToSet = utils.namehash(`${name}.os`);

      const data = await generateNetworkingKeys({
        direct,
        kns,
        nodeChainId,
        chainName,
        nameToSet,
        setNetworkingKey,
        setIpAddress,
        setWsPort,
        setTcpPort,
        setRouters,
      });

      const dnsFormat = toDNSWireFormat(`${name}.os`);
      const tx = await dotOs?.register(
        dnsFormat,
        accounts![0],
        data
      )

      setLoading('Registering KNS ID...');

      await tx?.wait();
      setLoading('');
      setOsName(`${name}.os`);
      navigate("/set-password");
    } catch (error) {
      console.error('Registration Error:', error)
      setLoading('');
      alert('There was an error registering your dot-os-name, please try again.')
    }
  }, [name, direct, accounts, dotOs, kns, navigate, setOsName, provider, openConnect, setNetworkingKey, setIpAddress, setWsPort, setTcpPort, setRouters, nodeChainId, chainName])

  return (
    <>
      <KinodeHeader header={<h1
        className="flex place-content-center place-items-center mb-4"
      >
        Register Kinode Name (KNS)
      </h1>}
        openConnect={openConnect}
        closeConnect={closeConnect}
        nodeChainId={nodeChainId}
      />
      {Boolean(provider) && <form
        id="signup-form"
        className="flex flex-col w-full max-w-[450px]"
        onSubmit={handleRegister}
      >
        {loading ? (
          <Loader msg={loading} />
        ) : (
          <>
            <h3 className="flex flex-col w-full place-items-center my-8">
              <label className="flex leading-6 place-items-center mt-2 cursor-pointer mb-2">
                Choose a name for your kinode
                <Tooltip text={`Kinodes use a .os name in order to identify themselves to other nodes in the network.`} />
              </label>
              <EnterKnsName {...enterOsNameProps} />
            </h3>
            <DirectCheckbox {...{ direct, setDirect }} />
            <button
              disabled={nameValidities.length !== 0}
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
      </form>}
    </>
  )
}

export default RegisterKnsName;
