import {
  FormEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
} from "react";
import { useNavigate } from "react-router-dom";
import { toAscii } from "idna-uts46-hx";
import isValidDomain from "is-valid-domain";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";
import { KINOMAP, MULTICALL, generateNetworkingKeys, kinomapAbi, mechAbi } from "../abis";
import { Tooltip } from "../components/Tooltip";
import DirectCheckbox from "../components/DirectCheckbox";
import EnterKnsName from "../components/EnterKnsName";

import { useAccount, usePublicClient, useWaitForTransactionReceipt, useWriteContract } from "wagmi";
import { useConnectModal } from "@rainbow-me/rainbowkit";
import { kinohash } from "../utils/kinohash";

const NAME_INVALID_PUNY = "Unsupported punycode character";
const NAME_NOT_OWNER = "Name does not belong to this wallet";
const NAME_NOT_REGISTERED = "Name is not registered";
const NAME_URL =
  "Name must be a valid URL without subdomains (A-Z, a-z, 0-9, and punycode)";


interface ResetProps extends PageProps { }

function Reset({
  direct,
  setDirect,
  setReset,
  knsName,
  setOsName,
  setNetworkingKey,
  setIpAddress,
  setWsPort,
  setTcpPort,
  setRouters,
  nodeChainId,
}: ResetProps) {
  const { address } = useAccount();
  const navigate = useNavigate();
  const client = usePublicClient();
  const { openConnectModal } = useConnectModal();

  const { data: hash, writeContract, isPending, isError, error } = useWriteContract();
  const { isLoading: isConfirming, isSuccess: isConfirmed } =
    useWaitForTransactionReceipt({
      hash,
    })

  const [name, setName] = useState<string>(knsName.slice(0, -3));
  const [nameVets, setNameVets] = useState<string[]>([]);
  const [nameValidities, setNameValidities] = useState<string[]>([])
  const [tba, setTba] = useState<string>("");
  const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false);


  useEffect(() => {
    document.title = "Reset";
  }, []);

  // so inputs will validate once wallet is connected
  useEffect(() => setTriggerNameCheck(!triggerNameCheck), [address]); // eslint-disable-line react-hooks/exhaustive-deps


  // TODO: separate this whole namechecking thing into helper function
  // boolean to branch whether to check for occupied or to match against our_address.

  const nameDebouncer = useRef<NodeJS.Timeout | null>(null);
  useEffect(() => {
    if (nameDebouncer.current) clearTimeout(nameDebouncer.current);

    nameDebouncer.current = setTimeout(async () => {
      setNameVets([]);


      if (name === "") return;

      let index: number;
      let vets = [...nameVets];

      let normalized: string;
      index = vets.indexOf(NAME_INVALID_PUNY);
      try {
        normalized = toAscii(name + ".os");
        if (index !== -1) vets.splice(index, 1);
      } catch (e) {
        if (index === -1) vets.push(NAME_INVALID_PUNY);
      }

      // only check if name is valid punycode
      if (normalized! !== undefined) {
        index = vets.indexOf(NAME_URL);
        if (name !== "" && !isValidDomain(normalized)) {
          if (index === -1) vets.push(NAME_URL);
        } else if (index !== -1) vets.splice(index, 1);

        try {
          const namehash = kinohash(normalized)
          console.log('normalized', normalized)
          console.log('namehash', namehash)
          // maybe separate into helper function for readability? 
          // also note picking the right chain ID & address!
          const data = await client?.readContract({
            address: KINOMAP,
            abi: kinomapAbi,
            functionName: "get",
            args: [namehash]
          })
          const tba = data?.[0];
          const owner = data?.[1];


          console.log('GOT data', data)
          console.log('GOT tba', tba)

          index = vets.indexOf(NAME_NOT_OWNER);
          if (owner === address && index !== -1) vets.splice(index, 1);
          else if (index === -1 && owner !== address)
            vets.push(NAME_NOT_OWNER);

          index = vets.indexOf(NAME_NOT_REGISTERED);
          if (index !== -1) vets.splice(index, 1);

          if (tba !== undefined) {
            setTba(tba);
          }
        } catch (e) {
          index = vets.indexOf(NAME_NOT_REGISTERED);
          if (index === -1) vets.push(NAME_NOT_REGISTERED);
        }

        if (nameVets.length === 0) setOsName(normalized);
      }

      setNameVets(vets);
    }, 500);
  }, [name, triggerNameCheck]); // eslint-disable-line react-hooks/exhaustive-deps

  const handleResetRecords = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (!address) {
        openConnectModal?.();
        return;
      }



      try {
        const data = await generateNetworkingKeys({
          direct,
          label: name,
          our_address: address,
          setNetworkingKey,
          setIpAddress,
          setWsPort,
          setTcpPort,
          setRouters,
          reset: true,
        });

        console.log('data', data)

        console.log('tba', tba)

        writeContract({
          address: tba as `0x${string}`,
          abi: mechAbi,
          functionName: "execute",
          args: [
            MULTICALL,
            BigInt(0),
            data,
            1
          ],
          gas: 1000000n,
        });
      } catch (error) {
        console.error("An error occurred:", error);
      }
    },
    [address, direct, tba, setNetworkingKey, setIpAddress, setWsPort, setTcpPort, setRouters, writeContract, openConnectModal]
  );

  useEffect(() => {
    if (isConfirmed) {
      setReset(true);
      setDirect(direct);
      navigate("/set-password");
    }
  }, [isConfirmed, setReset, setDirect, direct, navigate]);


  return (
    <>
      {Boolean(address) && (
        <form id="signup-form" className="flex flex-col" onSubmit={handleResetRecords}>
          {isPending || isConfirming ? (
            <Loader msg={isConfirming ? "Resetting Networking Information..." : "Please confirm the transaction in your wallet"} />
          ) : (
            <>
              <h3 className="flex flex-col w-full place-items-center mb-2">
                <label className="flex leading-6 place-items-center mt-2 cursor-pointer mb-2">
                  Specify the node ID to reset
                  <Tooltip text={`Kinodes use a .os name in order to identify themselves to other nodes in the network.`} />
                </label>
                <EnterKnsName {...{ name, setName, nameVets, triggerNameCheck, nameValidities, setNameValidities, isReset: true }} />
              </h3>

              <DirectCheckbox {...{ direct, setDirect }} />

              <button
                type="submit"
                className="mt-2"
                disabled={isPending || isConfirming || nameValidities.length !== 0}
              >
                Reset Node
              </button>
            </>
          )}
          {isError && (
            <p className="text-red-500 mt-2">
              Error: {error?.message || "An error occurred, please try again."}
            </p>
          )}
        </form>
      )}
    </>
  );
}
export default Reset;
