import { useState, useEffect, FormEvent, useCallback } from "react";
import { hooks } from "../connectors/metamask";
import { useNavigate } from "react-router-dom";
import { toDNSWireFormat } from "../utils/dnsWire";
import { utils } from "ethers";
import EnterEthName from "../components/EnterEthName";
import Loader from "../components/Loader";
import KinodeHeader from "../components/KnsHeader";
import { PageProps } from "../lib/types";
import { generateNetworkingKeys, getNetworkName, setChain } from "../utils/chain";
import { hash } from "eth-ens-namehash";
import DirectCheckbox from "../components/DirectCheckbox";
import { MAINNET_OPT_HEX, OPTIMISM_OPT_HEX } from "../constants/chainId";
import { KinodeTitle } from "../components/KinodeTitle";

const { useAccounts } = hooks;

interface RegisterOsNameProps extends PageProps { }

function RegisterEthName({
  direct,
  setDirect,
  setOsName,
  nameWrapper,
  ensRegistry,
  knsEnsEntry,
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
  const [loading, setLoading] = useState("");

  const [name, setName] = useState("");
  const [nameValidities, setNameValidities] = useState<string[]>([]);

  const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false);

  useEffect(() => {
    document.title = "Register";
  }, []);

  useEffect(() => setTriggerNameCheck(!triggerNameCheck), [provider]); // eslint-disable-line react-hooks/exhaustive-deps

  const enterEthNameProps = {
    name,
    setName,
    nameValidities,
    setNameValidities,
    nameWrapper,
    ensRegistry,
    triggerNameCheck,
  };

  let handleRegister = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (!provider) return openConnect();

      setLoading("Please confirm the transaction in your wallet");
      try {
        const cleanedName = name.trim().replace(".eth", "");
        const nameToSet = utils.namehash(`${cleanedName}.eth`);
        const targetChainId = nodeChainId === OPTIMISM_OPT_HEX ? MAINNET_OPT_HEX : nodeChainId;

        const data = await generateNetworkingKeys({
          direct,
          kns,
          nodeChainId: targetChainId,
          chainName,
          nameToSet,
          setNetworkingKey,
          setIpAddress,
          setWsPort,
          setTcpPort,
          setRouters,
        });

        setLoading("Please confirm the transaction in your wallet");

        // console.log("node chain id", nodeChainId);

        const dnsFormat = toDNSWireFormat(`${cleanedName}.eth`);
        const hashedName = hash(`${cleanedName}.eth`);

        const tx = await knsEnsEntry.setKNSRecords(dnsFormat, data, { gasLimit: 300000 });

        const onRegistered = (node: any, _name: any) => {
          if (node === hashedName) {
            kns.off("NodeRegistered", onRegistered);
            setLoading("");
            setOsName(`${cleanedName}.eth`);
            navigate("/set-password");
          }
        };

        await setChain(nodeChainId);

        setLoading(`Registering ${cleanedName}.eth on Kinode... this may take a few minutes.`);
        kns.on("NodeRegistered", onRegistered);
        await tx.wait();
      } catch (error) {
        console.error("Registration Error:", error);
        setLoading("");
        alert(
          "There was an error linking your ENS name, please try again."
        );
      }
    },
    [
      name,
      direct,
      accounts,
      kns,
      navigate,
      setOsName,
      provider,
      openConnect,
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
      <KinodeHeader
        header={<KinodeTitle prefix="Register via ENS" />}
        openConnect={openConnect}
        closeConnect={closeConnect}
        nodeChainId={nodeChainId === OPTIMISM_OPT_HEX ? MAINNET_OPT_HEX : nodeChainId}
      />
      {Boolean(provider) && (
        <form id="signup-form" className="flex flex-col" onSubmit={handleRegister}>
          {loading ? (
            <Loader msg={loading} />
          ) : (
            <>
              <h3 className="w-full flex flex-col c mb-2">
                <label className="flex leading-6 mb-2">
                  Set up your Kinode with a .eth name
                </label>
                <EnterEthName {...enterEthNameProps} />
              </h3>
              <DirectCheckbox {...{ direct, setDirect }} />
              <button
                disabled={nameValidities.length !== 0}
                type="submit"
                className="mt-2"
              >
                Register .eth name
              </button>
            </>
          )}
        </form>
      )}
    </>
  );
}

export default RegisterEthName;
