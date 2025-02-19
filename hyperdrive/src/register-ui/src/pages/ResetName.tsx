import {
  FormEvent,
  useCallback,
  useEffect,
  useState,
} from "react";
import { useNavigate } from "react-router-dom";
import Loader from "../components/Loader";
import { PageProps } from "../lib/types";
import { MULTICALL, generateNetworkingKeys, mechAbi } from "../abis";
import { Tooltip } from "../components/Tooltip";
import DirectCheckbox from "../components/DirectCheckbox";
import EnterHnsName from "../components/EnterHnsName";

import { useAccount, useWaitForTransactionReceipt, useWriteContract } from "wagmi";
import { useConnectModal, useAddRecentTransaction } from "@rainbow-me/rainbowkit";

interface ResetProps extends PageProps { }

function ResetHnsName({
  direct,
  setDirect,
  setReset,
  hnsName,
  setHnsName,
  setNetworkingKey,
  setIpAddress,
  setWsPort,
  setTcpPort,
  setRouters,
}: ResetProps) {
  const { address } = useAccount();
  const navigate = useNavigate();
  const { openConnectModal } = useConnectModal();

  const { data: hash, writeContract, isPending, isError, error } = useWriteContract({
    mutation: {
      onSuccess: (data) => {
        addRecentTransaction({ hash: data, description: `Reset HNS ID: ${name}` });
      }
    }
  });
  const { isLoading: isConfirming, isSuccess: isConfirmed } =
    useWaitForTransactionReceipt({
      hash,
    });
  const addRecentTransaction = useAddRecentTransaction();

  const [name, setName] = useState<string>(hnsName);
  const [nameValidities, setNameValidities] = useState<string[]>([])
  const [tba, setTba] = useState<string>("");
  const [triggerNameCheck, setTriggerNameCheck] = useState<boolean>(false);

  useEffect(() => {
    document.title = "Reset";
  }, []);

  // so inputs will validate once wallet is connected
  useEffect(() => setTriggerNameCheck(!triggerNameCheck), [address]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!address) {
      openConnectModal?.();
    }
  }, [address, openConnectModal]);

  const handleResetRecords = useCallback(
    async (e: FormEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (!address) {
        openConnectModal?.();
        return;
      }

      setHnsName(name);

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
    <div className="container fade-in">
      <button onClick={() => history.back()} className="button secondary back">🔙</button>
      <div className="section">
        {
          <form className="form" onSubmit={handleResetRecords}>
            {isPending || isConfirming ? (
              <Loader msg={isConfirming ? "Resetting Networking Information..." : "Please confirm the transaction in your wallet"} />
            ) : (
              <>
                <h3 className="form-label">
                  <Tooltip text="Nodes use an onchain username in order to identify themselves to other nodes in the network.">
                    Node ID to reset:
                  </Tooltip>
                </h3>
                <EnterHnsName {...{ address, name, setName, triggerNameCheck, nameValidities, setNameValidities, setTba, isReset: true }} />
                <DirectCheckbox {...{ direct, setDirect }} />
                <p>
                  A reset will not delete any data. It only updates the networking information that your node publishes onchain.
                </p>
                <button
                  type="submit"
                  className="button mt-2"
                  disabled={isPending || isConfirming || nameValidities.length !== 0}
                >
                  Reset Node
                </button>
              </>
            )}
            {isError && (
              <p className="error-message mt-2">
                Error: {error?.message || "An error occurred, please try again."}
              </p>
            )}
          </form>
        }
      </div>
    </div>
  );
}
export default ResetHnsName;
