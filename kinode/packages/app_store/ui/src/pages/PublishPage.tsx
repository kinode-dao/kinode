import React, { useState, useCallback, FormEvent, useEffect } from "react";
import { useLocation } from "react-router-dom";
import { useAccount, useWriteContract, useWaitForTransactionReceipt, usePublicClient } from 'wagmi'
import { ConnectButton, useConnectModal } from '@rainbow-me/rainbowkit';
import { keccak256, toBytes } from 'viem';
import { mechAbi, KIMAP, encodeIntoMintCall, encodeMulticalls, kimapAbi, MULTICALL } from "../abis";
import { kinohash } from '../utils/kinohash';
import useAppsStore from "../store";
import { AppInfo } from "../types/Apps";

export default function PublishPage() {
  const { state } = useLocation();
  const { openConnectModal } = useConnectModal();
  const { apps } = useAppsStore();
  const publicClient = usePublicClient();

  const { address, isConnected, isConnecting } = useAccount();
  const { data: hash, writeContract, error } = useWriteContract();
  const { isLoading: isConfirming, isSuccess: isConfirmed } =
    useWaitForTransactionReceipt({
      hash,
    });

  const [packageName, setPackageName] = useState<string>("");
  const [publisherId, setPublisherId] = useState<string>(window.our?.node || "");
  const [metadataUrl, setMetadataUrl] = useState<string>("");
  const [metadataHash, setMetadataHash] = useState<string>("");
  const [myPublishedApps, setMyPublishedApps] = useState<AppInfo[]>([]);

  useEffect(() => {
    const app: AppInfo | undefined = state?.app;
    if (app) {
      setPackageName(app.package);
      setPublisherId(app.publisher);
    }
  }, [state])

  useEffect(() => {
    setMyPublishedApps(
      apps.filter((app) => app.publisher?.toLowerCase() === window.our.node?.toLowerCase())
    );
  }, [apps, address])

  const calculateMetadataHash = useCallback(async () => {
    if (!metadataUrl) {
      setMetadataHash("");
      return;
    }

    try {
      const metadataResponse = await fetch(metadataUrl);
      const metadataText = await metadataResponse.text();
      JSON.parse(metadataText); // confirm it's valid JSON
      const metadataHash = keccak256(toBytes(metadataText));
      setMetadataHash(metadataHash);
    } catch (error) {
      alert("Error calculating metadata hash. Please ensure the URL is valid and the metadata is in JSON format.");
    }
  }, [metadataUrl]);

  const publishPackage = useCallback(
    async (e: FormEvent<HTMLFormElement>) => {
      e.preventDefault();
      e.stopPropagation();

      if (!publicClient || !address) {
        openConnectModal?.();
        return;
      }

      try {
        // Check if the package already exists and get its TBA
        let data = await publicClient.readContract({
          abi: kimapAbi,
          address: KIMAP,
          functionName: 'get',
          args: [kinohash(`${packageName}.${publisherId}`)]
        });

        let [tba, owner, _data] = data as [string, string, string];
        let isUpdate = Boolean(tba && tba !== '0x' && owner === address);
        let currentTBA = isUpdate ? tba as `0x${string}` : null;

        // If the package doesn't exist, check for the publisher's TBA
        if (!currentTBA) {
          data = await publicClient.readContract({
            abi: kimapAbi,
            address: KIMAP,
            functionName: 'get',
            args: [kinohash(publisherId)]
          });

          [tba, owner, _data] = data as [string, string, string];
          isUpdate = false; // It's a new package, but we might have a publisher TBA
          currentTBA = (tba && tba !== '0x') ? tba as `0x${string}` : null;
        }

        let metadata = metadataHash;
        if (!metadata) {
          const metadataResponse = await fetch(metadataUrl);
          await metadataResponse.json(); // confirm it's valid JSON
          const metadataText = await metadataResponse.text(); // hash as text
          metadata = keccak256(toBytes(metadataText));
        }

        const multicall = encodeMulticalls(metadataUrl, metadata);
        const args = isUpdate ? multicall : encodeIntoMintCall(multicall, address, packageName);

        writeContract({
          abi: mechAbi,
          address: currentTBA || KIMAP,
          functionName: 'execute',
          args: [
            isUpdate ? MULTICALL : KIMAP,
            BigInt(0),
            args,
            isUpdate ? 1 : 0
          ],
          gas: BigInt(1000000),
        });

        // Reset form fields
        setPackageName("");
        setPublisherId(window.our?.node || "");
        setMetadataUrl("");
        setMetadataHash("");

      } catch (error) {
        console.error(error);
      }
    },
    [publicClient, openConnectModal, packageName, publisherId, address, metadataUrl, metadataHash, writeContract]
  );

  const unpublishPackage = useCallback(
    async (packageName: string, publisherName: string) => {
      try {
        if (!publicClient) {
          openConnectModal?.();
          return;
        }

        const data = await publicClient.readContract({
          abi: kimapAbi,
          address: KIMAP,
          functionName: 'get',
          args: [kinohash(`${packageName}.${publisherName}`)]
        });

        const [tba, _owner, _data] = data as [string, string, string];

        if (!tba || tba === '0x') {
          console.error("No TBA found for this package");
          return;
        }

        const multicall = encodeMulticalls("", "");

        writeContract({
          abi: mechAbi,
          address: tba as `0x${string}`,
          functionName: 'execute',
          args: [
            KIMAP,
            BigInt(0),
            multicall,
            1
          ]
        });

      } catch (error) {
        console.error(error);
      }
    },
    [publicClient, openConnectModal, writeContract]
  );

  return (
    <div className="publish-page">
      <h1>Publish Package</h1>
      {Boolean(address) && (
        <div className="publisher-info">
          <span>Publishing as:</span>
          <span className="address">{address?.slice(0, 4)}...{address?.slice(-4)}</span>
        </div>
      )}

      {isConfirming ? (
        <div className="message info">Publishing package...</div>
      ) : !address || !isConnected ? (
        <>
          <h4>Please connect your wallet to publish a package</h4>
          <ConnectButton />
        </>
      ) : isConnecting ? (
        <div className="message info">Approve connection in your wallet</div>
      ) : (
        <form className="publish-form" onSubmit={publishPackage}>
          <div className="form-group">
            <label htmlFor="package-name">Package Name</label>
            <input
              id="package-name"
              type="text"
              required
              placeholder="my-package"
              value={packageName}
              onChange={(e) => setPackageName(e.target.value)}
            />
          </div>
          <div className="form-group">
            <label htmlFor="publisher-id">Publisher ID</label>
            <input
              id="publisher-id"
              type="text"
              required
              value={publisherId}
              onChange={(e) => setPublisherId(e.target.value)}
            />
          </div>
          <div className="form-group">
            <label htmlFor="metadata-url">Metadata URL</label>
            <input
              id="metadata-url"
              type="text"
              required
              value={metadataUrl}
              onChange={(e) => setMetadataUrl(e.target.value)}
              onBlur={calculateMetadataHash}
              placeholder="https://github/my-org/my-repo/metadata.json"
            />
            <p className="help-text">
              Metadata is a JSON file that describes your package.
            </p>
          </div>
          <div className="form-group">
            <label htmlFor="metadata-hash">Metadata Hash</label>
            <input
              readOnly
              id="metadata-hash"
              type="text"
              value={metadataHash}
              placeholder="Calculated automatically from metadata URL"
            />
          </div>
          <button type="submit" disabled={isConfirming}>
            {isConfirming ? 'Publishing...' : 'Publish'}
          </button>
        </form>
      )}

      {isConfirmed && (
        <div className="message success">
          Package published successfully!
        </div>
      )}
      {error && (
        <div className="message error">
          Error: {error.message}
        </div>
      )}

      <div className="my-packages">
        <h2>Packages You Own</h2>
        {myPublishedApps.length > 0 ? (
          <ul>
            {myPublishedApps.map((app) => (
              <li key={`${app.package}${app.publisher}`}>
                <span>{app.package}</span>
                <button onClick={() => unpublishPackage(app.package, app.publisher)}>
                  Unpublish
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p>No packages published</p>
        )}
      </div>
    </div>
  );
}