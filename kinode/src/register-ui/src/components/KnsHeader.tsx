import { useWeb3React } from "@web3-react/core";
import { hooks, metaMask } from "../connectors/metamask";
import { ReactNode, useCallback, useEffect, useState } from "react";
import Loader from "./Loader";
import { getNetworkName, setChain } from "../utils/chain";
import ChainInfo from "./ChainInfo";
import { OPTIMISM_OPT_HEX, SEPOLIA_OPT_HEX } from "../constants/chainId";

import sepoliaLogo from "../assets/sepolia.png";
import optimismLogo from "../assets/optimism.png";
import { Tooltip } from "./Tooltip";
import { isMobileCheck } from "../utils/dimensions";
import classNames from "classnames";

const { useIsActivating, useChainId } = hooks;

type OsHeaderProps = {
  header: ReactNode;
  nameLogo?: boolean;
  nodeChainId: string;
  openConnect: () => void;
  closeConnect: () => void;
  hideConnect?: boolean;
};

function KinodeHeader({
  header,
  closeConnect,
  nodeChainId,
  hideConnect = false,
}: OsHeaderProps) {
  const { account, isActive } = useWeb3React();
  const isActivating = useIsActivating();
  const chainId = useChainId();

  const [networkName, setNetworkName] = useState("");

  useEffect(() => {
    setNetworkName(getNetworkName((chainId || 1).toString()));
  }, [chainId]);

  const connectWallet = useCallback(async () => {
    closeConnect();
    await metaMask.activate().catch(() => { });

    try {
      setChain(nodeChainId);
    } catch (error) {
      console.error(error);
    }
  }, [closeConnect, nodeChainId]);

  const changeToNodeChain = useCallback(async () => {
    // If correct ndetwork is set, just say that
    if (chainId) {
      const hexChainId = "0x" + chainId.toString(16);
      if (hexChainId === nodeChainId) {
        return alert(
          `You are already connected to ${getNetworkName(chainId.toString())}`
        );
      }

      try {
        setChain(nodeChainId);
      } catch (error) {
        console.error(error);
      }
    }
  }, [chainId, nodeChainId]);

  const changeConnectedAccount = useCallback(async () => {
    alert("You can change your connected account in your wallet.");
  }, []);

  const isMobile = isMobileCheck()

  return (
    <>
      <div id="signup-form-header" className="flex flex-col">
        {(nodeChainId === SEPOLIA_OPT_HEX ||
          nodeChainId === OPTIMISM_OPT_HEX) && (
            <Tooltip
              position="left"
              className={classNames("!absolute z-10", {
                'top-8 right-8': !isMobile,
                'top-2 right-2': isMobile
              })}
              button={nodeChainId === SEPOLIA_OPT_HEX ? (
                <img
                  alt="sepolia"
                  className="network-icon"
                  src={sepoliaLogo}
                />
              ) : nodeChainId === OPTIMISM_OPT_HEX ? (
                <img
                  alt="optimism"
                  className="network-icon"
                  src={optimismLogo}
                />
              ) : null}
              text={nodeChainId === SEPOLIA_OPT_HEX
                ? `Your Kinode is currently pointed at Sepolia. To point at Optimism, boot without the "--testnet" flag.`
                : nodeChainId === OPTIMISM_OPT_HEX
                  ? `Your Kinode is currently pointed at Optimism. To point at Sepolia, boot with the "--testnet" flag.`
                  : ''}
            />
          )}
        <div className="flex flex-col gap-4 c">
          {header}
        </div>
        {!hideConnect && (
          <div
            className="flex c w-[99vw] mb-8 absolute top-2 left-2"
          >
            {isActive && account ? (
              <ChainInfo
                account={account}
                networkName={networkName}
                changeToNodeChain={changeToNodeChain}
                changeConnectedAccount={changeConnectedAccount}
              />
            ) : (
              <div className="flex flex-col gap-8 my-4">
                <h5 className={classNames("flex c", {
                  'flex-wrap text-center max-w-3/4 gap-2': isMobile
                })}>
                  {!isActivating && 'You must connect to a browser wallet to continue.'}

                  {isActivating ? (
                    <Loader msg="Approve connection in your wallet" />
                  ) : (
                    <button onClick={connectWallet} className="ml-2"> Connect Wallet </button>
                  )}
                </h5>
                {nodeChainId === SEPOLIA_OPT_HEX && (
                  <h5
                    className="text-center max-w-[450px] leading-6 flex c"
                  >
                    Kinode is currently on the Sepolia Testnet.
                    <a
                      href="https://sepoliafaucet.com/"
                      target="_blank"
                      rel="noreferrer"
                      className="button alt ml-2"
                    >
                      Get Testnet ETH
                    </a>
                  </h5>
                )}
              </div>
            )}
          </div>
        )}
      </div>
    </>
  );
}

export default KinodeHeader;
