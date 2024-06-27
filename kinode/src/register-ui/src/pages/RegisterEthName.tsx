import { useState, useEffect, FormEvent, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import EnterEthName from "../components/EnterEthName";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";
import { hash } from "@ensdomains/eth-ens-namehash";
import DirectCheckbox from "../components/DirectCheckbox";

import { useAccount } from "wagmi";

interface RegisterOsNameProps extends PageProps { }

function RegisterEthName({
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

  const [loading, setLoading] = useState("");

  const [name, setName] = useState("");
  const [nameValidities, setNameValidities] = useState<string[]>([]);

  const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false);

  useEffect(() => {
    document.title = "Register";
  }, []);

  useEffect(() => setTriggerNameCheck(!triggerNameCheck), [address]); // eslint-disable-line react-hooks/exhaustive-deps

  const enterEthNameProps = {
    name,
    setName,
    nameValidities,
    setNameValidities,
    triggerNameCheck,
  };

  let handleRegister = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      e.stopPropagation();


      setLoading("Please confirm the transaction in your wallet");
      try {
        const cleanedName = name.trim().replace(".eth", "");
        const nameToSet = "namehash(`${cleanedName}.eth`)";

        // const data = await generateNetworkingKeys({
        //   direct,
        //   kns: "kns here",
        //   nodeChainId: targetChainId,
        //   chainName,
        //   nameToSet,
        //   setNetworkingKey,
        //   setIpAddress,
        //   setWsPort,
        //   setTcpPort,
        //   setRouters,
        // });

        setLoading("Please confirm the transaction in your wallet");

        // console.log("node chain id", nodeChainId);

        const hashedName = hash(`${cleanedName}.eth`);

        // const tx = await knsEnsEntry.setKNSRecords(dnsFormat, data, { gasLimit: 300000 });

        // const onRegistered = (node: any, _name: any) => {
        //   if (node === hashedName) {
        //     kns.off("NodeRegistered", onRegistered);
        //     setLoading("");
        //     setOsName(`${cleanedName}.eth`);
        //     navigate("/set-password");
        //   }
        // };

        // await setChain(nodeChainId);

        // setLoading(`Registering ${cleanedName}.eth on Kinode... this may take a few minutes.`);
        // kns.on("NodeRegistered", onRegistered);
        // await tx.wait();
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
      navigate,
      setOsName,
      setNetworkingKey,
      setIpAddress,
      setWsPort,
      setTcpPort,
      setRouters,
      nodeChainId,
    ]
  );

  return (
    <>

      {Boolean(address) && (
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
