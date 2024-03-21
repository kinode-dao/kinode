import { useWeb3React } from "@web3-react/core";
import { hooks, metaMask } from "../connectors/metamask";
import { ReactNode, useCallback, useEffect, useState } from "react";
import Loader from "./Loader";
import { getNetworkName, setChain } from "../utils/chain";
import ChainInfo from "./ChainInfo";
import { OPTIMISM_OPT_HEX, SEPOLIA_OPT_HEX } from "../constants/chainId";

import sepoliaLogo from "../assets/sepolia.png";
import optimismLogo from "../assets/optimism.png";

const { useIsActivating, useChainId } = hooks;

type OsHeaderProps = {
  header: ReactNode;
  nameLogo?: boolean;
  nodeChainId: string;
  openConnect: () => void;
  closeConnect: () => void;
  hideConnect?: boolean;
};

function OsHeader({
  header,
  openConnect,
  nameLogo = false,
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
    await metaMask.activate().catch(() => {});

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

  // <div style={{ textAlign: 'center', lineHeight: 1.5 }}> Connected as {account?.slice(0,6) + '...' + account?.slice(account.length - 6)}</div>
  return (
    <>
      <div id="signup-form-header" className="col">
        {(nodeChainId === SEPOLIA_OPT_HEX ||
          nodeChainId === OPTIMISM_OPT_HEX) && (
          <div
            className="tooltip-container"
            style={{ position: "absolute", top: 32, right: 32 }}
          >
            <div className="tooltip-button chain">
              {nodeChainId === SEPOLIA_OPT_HEX ? (
                <img alt="sepolia" src={sepoliaLogo} className="sepolia" />
              ) : nodeChainId === OPTIMISM_OPT_HEX ? (
                <img alt="optimism" src={optimismLogo} />
              ) : null}
            </div>
            <div className="tooltip-content left">
              {nodeChainId === SEPOLIA_OPT_HEX ? (
                <div
                  style={{
                    textAlign: "center",
                    lineHeight: "1.5em",
                    maxWidth: 450,
                  }}
                >
                  Your Kinode is currently pointed at Sepolia. To point at
                  Optimism, boot without the "--testnet" flag.
                </div>
              ) : nodeChainId === OPTIMISM_OPT_HEX ? (
                <div
                  style={{
                    textAlign: "center",
                    lineHeight: "1.5em",
                    maxWidth: 450,
                  }}
                >
                  Your Kinode is currently pointed at Optimism. To point at
                  Sepolia, boot with the "--testnet" flag.
                </div>
              ) : null}
            </div>
          </div>
        )}
        <div className="col" style={{ gap: 16, marginBottom: 32 }}>
          {header}
        </div>
        {!hideConnect && (
          <div
            style={{
              minWidth: "50vw",
              width: 400,
              justifyContent: "center",
              display: "flex",
            }}
          >
            {isActive && account ? (
              <ChainInfo
                account={account}
                networkName={networkName}
                changeToNodeChain={changeToNodeChain}
                changeConnectedAccount={changeConnectedAccount}
              />
            ) : (
              <div className="col" style={{ gap: 32, marginTop: 16 }}>
                <h5 style={{ textAlign: "center", lineHeight: "1.5em" }}>
                  You must connect to a browser wallet to continue
                </h5>
                {/* <div style={{ textAlign: 'center', lineHeight: '1.5em' }}>We recommend <a href="https://metamask.io/download.html" target="_blank" rel="noreferrer">MetaMask</a></div> */}
                {isActivating ? (
                  <Loader msg="Approve connection in your wallet" />
                ) : (
                  <button onClick={connectWallet}> Connect Wallet </button>
                )}
                {nodeChainId === SEPOLIA_OPT_HEX && (
                  <h5
                    style={{
                      textAlign: "center",
                      lineHeight: "1.5em",
                      maxWidth: 450,
                    }}
                  >
                    Kinode is currently on the Sepolia Testnet, if you need
                    testnet ETH, you can get some from the{" "}
                    <a
                      href="https://sepoliafaucet.com/"
                      target="_blank"
                      rel="noreferrer"
                    >
                      Sepolia Faucet
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

export default OsHeader;
