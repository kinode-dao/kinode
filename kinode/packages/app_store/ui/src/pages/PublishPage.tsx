import React, { useState, useCallback, FormEvent, useEffect } from "react";
import { useLocation } from "react-router-dom";
import { useAccount, useWriteContract, useWaitForTransactionReceipt, usePublicClient } from 'wagmi'
import { ConnectButton, useConnectModal } from '@rainbow-me/rainbowkit';
import { keccak256, toBytes } from 'viem';
import { mechAbi, KINOMAP, encodeIntoMintCall, encodeMulticalls, kinomapAbi, MULTICALL } from "../abis";
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
  const [isUpdate, setIsUpdate] = useState<boolean>(false);
  const [myPublishedApps, setMyPublishedApps] = useState<AppInfo[]>([]);

  useEffect(() => {
    const app: AppInfo | undefined = state?.app;
    if (app) {
      setPackageName(app.package);
      setPublisherId(app.publisher);
      setIsUpdate(true);
    }
  }, [state])

  useEffect(() => {
    setMyPublishedApps(
      apps.filter((app) => app.owner?.toLowerCase() === address?.toLowerCase())
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

      if (!publicClient) {
        openConnectModal?.();
        return;
      }

      let node = window.our?.node || "0x";
      let metadata = metadataHash;

      if (isUpdate) {
        node = `${packageName}.${window.our?.node || "0x"}`;
      }

      try {
        let data = await publicClient.readContract({
          abi: kinomapAbi,
          address: KINOMAP,
          functionName: 'get',
          args: [kinohash(node)]
        });

        if (!metadata) {
          const metadataResponse = await fetch(metadataUrl);
          await metadataResponse.json(); // confirm it's valid JSON
          const metadataText = await metadataResponse.text(); // hash as text
          metadata = keccak256(toBytes(metadataText));
        }

        const multicall = encodeMulticalls(metadataUrl, metadata);
        const args = isUpdate ? multicall : encodeIntoMintCall(multicall, address!, packageName);

        const [tba, _owner, _data] = data || [];

        writeContract({
          abi: mechAbi,
          address: tba as `0x${string}`,
          functionName: 'execute',
          args: [
            isUpdate ? MULTICALL : KINOMAP,
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
        setIsUpdate(false);

      } catch (error) {
        console.error(error);
      }
    },
    [publicClient, openConnectModal, packageName, publisherId, address, metadataUrl, metadataHash, isUpdate, writeContract]
  );

  const unpublishPackage = useCallback(
    async (packageName: string, publisherName: string) => {
      try {
        if (!publicClient) {
          openConnectModal?.();
          return;
        }

        const node = `${packageName}.${window.our?.node || "0x"}`;
        const nodehash = kinohash(node);

        const data = await publicClient.readContract({
          abi: kinomapAbi,
          address: KINOMAP,
          functionName: 'get',
          args: [nodehash]
        });

        const [tba, _owner, _data] = data || [];

        const multicall = encodeMulticalls("", "");

        writeContract({
          abi: mechAbi,
          address: tba as `0x${string}`,
          functionName: 'execute',
          args: [
            KINOMAP,
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

  const checkIfUpdate = useCallback(() => {
    if (isUpdate) return;

    if (
      packageName &&
      publisherId &&
      apps.find(
        (app) => app.package === packageName && app.publisher === publisherId
      )
    ) {
      setIsUpdate(true);
    }
  }, [apps, packageName, publisherId, isUpdate]);

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
            <input
              type="checkbox"
              id="update"
              checked={isUpdate}
              onChange={() => setIsUpdate(!isUpdate)}
            />
            <label htmlFor="update">Update existing package</label>
          </div>
          <div className="form-group">
            <label htmlFor="package-name">Package Name</label>
            <input
              id="package-name"
              type="text"
              required
              placeholder="my-package"
              value={packageName}
              onChange={(e) => setPackageName(e.target.value)}
              onBlur={checkIfUpdate}
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
              onBlur={checkIfUpdate}
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
          Package {isUpdate ? 'updated' : 'published'} successfully!
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