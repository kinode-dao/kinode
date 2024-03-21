import React, { useState, useEffect, FormEvent, useCallback } from "react";
import { hooks } from "../connectors/metamask";
import { Link, useNavigate } from "react-router-dom";
import { toDNSWireFormat } from "../utils/dnsWire";
import { BytesLike, utils } from "ethers";
import EnterEthName from "../components/EnterEthName";
import Loader from "../components/Loader";
import OsHeader from "../components/KnsHeader";
import { NetworkingInfo, PageProps } from "../lib/types";
import { ipToNumber } from "../utils/ipToNumber";
import { getNetworkName, setChain } from "../utils/chain";
import { hash } from "eth-ens-namehash";
import { ReactComponent as NameLogo } from "../assets/kinode.svg";
import DirectCheckbox from "../components/DirectCheckbox";
import { MAINNET_OPT_HEX, OPTIMISM_OPT_HEX } from "../constants/chainId";

const { useAccounts } = hooks;

interface RegisterOsNameProps extends PageProps { }

function RegisterEthName({
  direct,
  setDirect,
  setOsName,
  nameWrapper,
  ensRegistry,
  knsEnsEntry,
  knsEnsExit,
  kns,
  openConnect,
  provider,
  closeConnect,
  setNetworkingKey,
  setIpAddress,
  setPort,
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

      try {
        setLoading("Please confirm the transaction in your wallet");

        const {
          networking_key,
          ws_routing: [ip_address, port],
          allowed_routers,
        } = (await fetch("/generate-networking-info", { method: "POST" }).then(
          (res) => res.json()
        )) as NetworkingInfo;

        const ipAddress = ipToNumber(ip_address);

        setNetworkingKey(networking_key);
        setIpAddress(ipAddress);
        setPort(port);
        setRouters(allowed_routers);

        const cleanedName = name.trim().replace(".eth", "");

        const targetChainId = nodeChainId === OPTIMISM_OPT_HEX ? MAINNET_OPT_HEX : nodeChainId;

        try {
          await setChain(targetChainId);
        } catch (error) {
          window.alert(
            `You must connect to the ${getNetworkName(targetChainId)} network to continue. Please connect and try again.`
          );
          throw new Error(`${getNetworkName(targetChainId)} not connected`);
        }

        const data: BytesLike[] = [
          direct
            ? (
              await kns.populateTransaction.setAllIp(
                utils.namehash(`${cleanedName}.eth`),
                ipAddress,
                port,
                0,
                0,
                0
              )
            ).data!
            : (
              await kns.populateTransaction.setRouters(
                utils.namehash(`${cleanedName}.eth`),
                allowed_routers.map((x) => utils.namehash(x))
              )
            ).data!,
          (
            await kns.populateTransaction.setKey(
              utils.namehash(`${cleanedName}.eth`),
              networking_key
            )
          ).data!,
        ];

        setLoading("Please confirm the transaction in your wallet");

        // console.log("node chain id", nodeChainId);

        const dnsFormat = toDNSWireFormat(`${cleanedName}.eth`);
        const namehash = hash(`${cleanedName}.eth`);

        const tx = await knsEnsEntry.setKNSRecords(dnsFormat, data, { gasLimit: 300000 });

        const onRegistered = (node: any, name: any) => {
          if (node === namehash) {
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
      setPort,
      setRouters,
      nodeChainId,
      chainName,
    ]
  );

  return (
    <>
      <OsHeader
        header={
          <h3
            className="row"
            style={{ justifyContent: "center", alignItems: "center" }}
          >
            Register
            <NameLogo
              style={{ height: 28, width: "auto", margin: "0 16px -3px" }}
            />
            Name
          </h3>
        }
        openConnect={openConnect}
        closeConnect={closeConnect}
        nodeChainId={nodeChainId === OPTIMISM_OPT_HEX ? MAINNET_OPT_HEX : nodeChainId}
      />
      {Boolean(provider) && (
        <form id="signup-form" className="col" onSubmit={handleRegister}>
          {loading ? (
            <Loader msg={loading} />
          ) : (
            <>
              <div style={{ width: "100%" }}>
                <label className="login-row row" style={{ lineHeight: 1.5 }}>
                  Set up your Kinode with a .eth name
                </label>
                <EnterEthName {...enterEthNameProps} />
              </div>
              <DirectCheckbox {...{ direct, setDirect }} />
              <button disabled={nameValidities.length !== 0} type="submit">
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
