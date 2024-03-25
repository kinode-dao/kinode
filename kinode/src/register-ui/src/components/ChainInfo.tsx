import { useCallback } from 'react';
import ethLogo from '../assets/eth.png';
import sepoliaLogo from '../assets/sepolia.png';
import optimismLogo from '../assets/optimism.png';
import arbitrumLogo from '../assets/arbitrum.png';
import unknownLogo from '../assets/unknown.png';
import Jazzicon from "./Jazzicon";
import { hooks } from "../connectors/metamask";
import { KNS_REGISTRY_ADDRESSES } from '../constants/addresses';

const { useChainId } = hooks;

interface ChainInfoProps {
  account: string;
  networkName: string;
  changeConnectedAccount: () => void;
  changeToNodeChain: () => void;
}

function ChainInfo({
  account,
  networkName,
  changeConnectedAccount,
  changeToNodeChain,
}: ChainInfoProps) {
  const chainId = useChainId();

  const formatAddress = (address: string) => {
    return `${address.substring(0, 6)}...${address.substring(
      address.length - 4
    )}`;
  };

  const generateNetworkIcon = (networkName: string) => {
    switch (networkName) {
      case "Ethereum":
        return <img className="network-icon" src={ethLogo} alt={networkName} />;
      case "Optimism":
        return (
          <img className="network-icon" src={optimismLogo} alt={networkName} />
        );
      case "Arbitrum":
        return (
          <img className="network-icon" src={arbitrumLogo} alt={networkName} />
        );
      case "Sepolia":
        return (
          <img
            className="network-icon"
            src={sepoliaLogo}
            alt={networkName}
            style={{ filter: "grayscale(100%)" }}
          />
        );
      default:
        return (
          <img
            className="network-icon"
            src={unknownLogo}
            alt={networkName}
            style={{ filter: "grayscale(100%)" }}
          />
        );
    }
  };

  const showKnsAddress = useCallback(() => {
    window.alert(`The KNS Contract Address is: ${KNS_REGISTRY_ADDRESSES[chainId || ''] || 'unavailable on ' + networkName}`)
  }, [chainId, networkName])

  return (
    <div style={{ display: "flex", gap: 10, maxWidth: 500 }}>
      {/* TODO: prompt to change address */}
      <button
        onClick={changeConnectedAccount}
        className="chain-button monospace"
      >
        <Jazzicon
          address={account || ""}
          diameter={24}
          style={{ marginRight: "0.5em" }}
        />{" "}
        {formatAddress(account || "")}
      </button>
      <button
        onClick={changeToNodeChain}
        className="chain-button"
        style={{ maxWidth: "27%" }}
      >
        {generateNetworkIcon(networkName)} {networkName}
      </button>
      {/* TODO: show KNS contract ID in modal */}
      <button onClick={showKnsAddress} className="chain-button" style={{ maxWidth: "27%" }}>
        KNS Contract
      </button>
    </div>
  );
}

export default ChainInfo;
