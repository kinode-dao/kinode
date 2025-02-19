import React, { useState, useCallback, FormEvent, useEffect } from "react";
import { Link, useLocation } from "react-router-dom";
import { useAccount, useWriteContract, useWaitForTransactionReceipt, usePublicClient } from 'wagmi'
import { ConnectButton, useConnectModal } from '@rainbow-me/rainbowkit';
import { keccak256, toBytes } from 'viem';
import { mechAbi, HYPERMAP, encodeIntoMintCall, encodeMulticalls, hypermapAbi, MULTICALL } from "../abis";
import { kinohash } from '../utils/kinohash';
import useAppsStore from "../store";
import { PackageSelector } from "../components";
import { Tooltip } from '../components/Tooltip';

const NAME_INVALID = "Package name must contain only valid characters (a-z, 0-9, -, and .)";

export default function PublishPage() {
  const { openConnectModal } = useConnectModal();
  const { ourApps, fetchOurApps, downloads, fetchDownloadsForApp } = useAppsStore();
  const publicClient = usePublicClient();

  const { address, isConnected, isConnecting } = useAccount();
  const { data: hash, writeContract, error } = useWriteContract();
  const { isLoading: isConfirming, isSuccess: isConfirmed } =
    useWaitForTransactionReceipt({
      hash,
    });

  const [packageName, setPackageName] = useState<string>("");
  // @ts-ignore
  const [publisherId, setPublisherId] = useState<string>(window.our?.node || "");
  const [metadataUrl, setMetadataUrl] = useState<string>("");
  const [metadataHash, setMetadataHash] = useState<string>("");

  const [nameValidity, setNameValidity] = useState<string | null>(null);
  const [metadataError, setMetadataError] = useState<string | null>(null);

  useEffect(() => {
    fetchOurApps();
  }, [fetchOurApps]);

  useEffect(() => {
    if (packageName && publisherId) {
      const id = `${packageName}:${publisherId}`;
      fetchDownloadsForApp(id);
    }
  }, [packageName, publisherId, fetchDownloadsForApp]);

  useEffect(() => {
    if (isConfirmed) {
      // Fetch our apps again after successful publish
      fetchOurApps();
      // Reset form fields
      setPackageName("");
      // @ts-ignore
      setPublisherId(window.our?.node || "");
      setMetadataUrl("");
      setMetadataHash("");
    }
  }, [isConfirmed, fetchOurApps]);

  const validatePackageName = useCallback((name: string) => {
    // Allow lowercase letters, numbers, hyphens, and dots
    const validNameRegex = /^[a-z0-9.-]+$/;

    if (!validNameRegex.test(name)) {
      setNameValidity(NAME_INVALID);
    } else {
      setNameValidity(null);
    }
  }, []);

  useEffect(() => {
    if (packageName) {
      validatePackageName(packageName);
    } else {
      setNameValidity(null);
    }
  }, [packageName, validatePackageName]);


  const calculateMetadataHash = useCallback(async () => {
    if (!metadataUrl) {
      setMetadataHash("");
      setMetadataError("");
      return;
    }

    try {
      const metadataResponse = await fetch(metadataUrl);
      const metadataText = await metadataResponse.text();
      const metadata = JSON.parse(metadataText);

      // Check if code_hashes exist in metadata and is an object
      if (metadata.properties && metadata.properties.code_hashes && typeof metadata.properties.code_hashes === 'object') {
        const codeHashes = metadata.properties.code_hashes;
        console.log('Available downloads:', downloads[`${packageName}:${publisherId}`]);

        const missingHashes = Object.entries(codeHashes).filter(([version, hash]) => {
          const hasDownload = downloads[`${packageName}:${publisherId}`]?.some(d => d.File?.name === `${hash}.zip`);
          return !hasDownload;
        });

        if (missingHashes.length == codeHashes.length) {
          setMetadataError(`Missing local downloads for mirroring versions: ${missingHashes.map(([version]) => version).join(', ')}`);
        } else {
          setMetadataError("");
        }
      } else {
        setMetadataError("The metadata does not contain the required 'code_hashes' property or it is not in the expected format");
      }

      const metadataHash = keccak256(toBytes(metadataText));
      setMetadataHash(metadataHash);
    } catch (error) {
      if (error instanceof SyntaxError) {
        setMetadataError("The metadata is not valid JSON. Please check the file for syntax errors.");
      } else if (error instanceof Error) {
        setMetadataError(`Error processing metadata: ${error.message}`);
      } else {
        setMetadataError("An unknown error occurred while processing the metadata.");
      }
      setMetadataHash("");
    }
  }, [metadataUrl, packageName, publisherId, downloads]);

  const handlePackageSelection = (packageName: string, publisherId: string) => {
    setPackageName(packageName);
    setPublisherId(publisherId);
  };

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
        console.log('packageName, publisherId: ', packageName, publisherId)
        let data = await publicClient.readContract({
          abi: hypermapAbi,
          address: HYPERMAP,
          functionName: 'get',
          args: [kinohash(`${packageName}.${publisherId}`)]
        });

        let [tba, owner, _data] = data as [string, string, string];
        let isUpdate = Boolean(tba && tba !== '0x' && owner === address);
        let currentTBA = isUpdate ? tba as `0x${string}` : null;
        console.log('currenttba, isupdate, owner, address: ', currentTBA, isUpdate, owner, address)
        // If the package doesn't exist, check for the publisher's TBA
        if (!currentTBA) {
          data = await publicClient.readContract({
            abi: hypermapAbi,
            address: HYPERMAP,
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
          address: currentTBA || HYPERMAP,
          functionName: 'execute',
          args: [
            isUpdate ? MULTICALL : HYPERMAP,
            BigInt(0),
            args,
            isUpdate ? 1 : 0
          ],
          gas: BigInt(1000000),
        });

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
          abi: hypermapAbi,
          address: HYPERMAP,
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
            MULTICALL,
            BigInt(0),
            multicall,
            1
          ],
          gas: BigInt(1000000),
        });

      } catch (error) {
        console.error(error);
      }
    },
    [publicClient, openConnectModal, writeContract]
  );

  return (
    <div className="publish-page">
      <h1>Manage Published Apps</h1>
      {!address ? <></> : (
        <div className="wallet-status">
          Connected: {address}
          <Tooltip content="Make sure the connected wallet is the owner of this node!" />
        </div>
      )}

      {Object.keys(ourApps).length === 0 ? (
        <a href="https://book.hyperware.ai/my_first_app/chapter_5.html" target="_blank">
          First time? Read the guide on publishing an app here.
        </a>
      ) : (
        <div className="my-packages">
          <h2>Your Published Apps</h2>
          <ul className="package-list">
            {Object.values(ourApps).map((app) => (
              <li key={`${app.package_id.package_name}:${app.package_id.publisher_node}`}>
                <Link to={`/app/${app.package_id.package_name}:${app.package_id.publisher_node}`} className="app-name">
                  {app.metadata?.image && (
                    <img src={app.metadata.image} alt="" className="package-icon" />
                  )}
                  <span>{app.metadata?.name || app.package_id.package_name}</span>
                </Link>

                <button onClick={() => unpublishPackage(app.package_id.package_name, app.package_id.publisher_node)} className="danger">
                  Unpublish
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {isConfirming ? (
        <div className="message info">
          <div className="loading-spinner"></div>
          <span>Publishing...</span>
        </div>
      ) : !address || !isConnected ? (
        <div className="connect-wallet">
          <h4>Connect your wallet to publish an app.</h4>
        </div>
      ) : isConnecting ? (
        <div className="message info">
          <div className="loading-spinner"></div>
          <span>Waiting for wallet connection...</span>
        </div>
      ) : (
        <form className="publish-form" onSubmit={publishPackage}>
          <div className="form-group">
            <label htmlFor="package-select">Select app to publish</label>
            <PackageSelector onPackageSelect={handlePackageSelection} publisherId={publisherId} />
            {nameValidity && <p className="error-message">{nameValidity}</p>}
          </div>

          <div className="form-group">
            <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
              <label>Metadata URL</label>
              <Tooltip content={<>add a link to metadata.json here (<a href="https://raw.githubusercontent.com/hyperware-ai/kit/47cdf82f70b36f2a102ddfaaeed5efa10d7ef5b9/src/new/templates/rust/ui/chat/metadata.json" target="_blank" rel="noopener noreferrer">example link</a>)</>} />
            </div>
            <input
              type="text"
              value={metadataUrl}
              onChange={(e) => setMetadataUrl(e.target.value)}
              onBlur={calculateMetadataHash}
            />
            {metadataError && <p className="error-message">{metadataError}</p>}
          </div>
          <div className="form-group">
            <label>Metadata Hash</label>
            <input
              readOnly
              type="text"
              value={metadataHash}
              placeholder="Calculated automatically from metadata URL"
            />
          </div>
          <button type="submit" disabled={isConfirming || nameValidity !== null || Boolean(metadataError)}>
            {isConfirming ? (
              <>
                <div className="loading-spinner small"></div>
                <span>Publishing...</span>
              </>
            ) : (
              'Publish'
            )}
          </button>
        </form>
      )}

      {isConfirmed && (
        <div className="message success">
          App published successfully!
        </div>
      )}
      {error && (
        <div className="message error">
          <pre>
            Error: {error.message}
          </pre>
        </div>
      )}
    </div>
  );
}