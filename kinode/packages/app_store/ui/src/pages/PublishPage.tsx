import React, { useState, useCallback, FormEvent, useEffect } from "react";
import { useLocation } from "react-router-dom";

import { useAccount, useWriteContract, useWaitForTransactionReceipt, usePublicClient } from 'wagmi'
import { ConnectButton, useConnectModal } from '@rainbow-me/rainbowkit';
import { keccak256, toBytes } from 'viem';
import { mechAbi, KINOMAP, encodeIntoMintCall, encodeMulticalls, kinomapAbi, MULTICALL } from "../abis";
import { kinohash } from '../utils/kinohash';

import {
  SearchHeader,
  Jazzicon,
  Loader,
  MetadataForm,
  Checkbox,
  Tooltip,
  HomeButton,
  MessagePopup
} from "../components";
import useAppsStore from "../store/apps-store";
import { AppInfo } from "../types/Apps";
import classNames from "classnames";
import { isMobileCheck } from "../utils/dimensions";


export default function PublishPage() {
  const { state } = useLocation();
  const { openConnectModal } = useConnectModal();
  const { listedApps } = useAppsStore();
  const publicClient = usePublicClient();

  const { address, isConnected, isConnecting } = useAccount();
  const { data: hash, writeContract, error } = useWriteContract();
  const { isLoading: isConfirming, isSuccess: isConfirmed } =
    useWaitForTransactionReceipt({
      hash,
    });

  // single state for displaying messages
  const [showMetadataForm, setShowMetadataForm] = useState<boolean>(false);

  const [packageName, setPackageName] = useState<string>("");
  const [publisherId, setPublisherId] = useState<string>(
    window.our?.node || ""
  );

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
      listedApps.filter((app) => app.owner?.toLowerCase() === address?.toLowerCase())
    );
  }, [listedApps, address])

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
      window.alert(
        "Error calculating metadata hash. Please ensure the URL is valid and the metadata is in JSON format."
      );
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

      console.log('we are publishing... with node, and isUpdate: ', node, isUpdate)


      try {
        let data = await publicClient.readContract({
          abi: kinomapAbi,
          address: KINOMAP,
          functionName: 'get',
          args: [kinohash(node)]
        });

        console.log('node:', node, 'publisherId:', publisherId, 'address:', address, 'node:', node, 'data:', data);

        if (!metadata) {
          const metadataResponse = await fetch(metadataUrl);
          await metadataResponse.json(); // confirm it's valid JSON
          const metadataText = await metadataResponse.text(); // hash as text
          metadata = keccak256(toBytes(metadataText));
        }

        const multicall = encodeMulticalls(metadataUrl, metadata);
        const args = isUpdate ? multicall : encodeIntoMintCall(multicall, address!, packageName);

        const [tba, _owner, _data] = data || [];

        console.log('tba: ', tba);


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
    [
      publicClient,
      openConnectModal,
      packageName,
      publisherId,
      address,
      metadataUrl,
      metadataHash,
      isUpdate,
      writeContract,
      setPackageName,
      setPublisherId,
      setMetadataUrl,
      setMetadataHash,
      setIsUpdate,
    ]
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
        console.log('node:', window.our?.node, 'publisherId:', publisherId, 'address:', address, 'node:', node, 'data:', data);

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
    [publicClient, openConnectModal, writeContract, publisherId, address]
  );


  const checkIfUpdate = useCallback(async () => {
    if (isUpdate) return;

    if (
      packageName &&
      publisherId &&
      listedApps.find(
        (app) => app.package === packageName && app.publisher === publisherId
      )
    ) {
      setIsUpdate(true);
    }
  }, [listedApps, packageName, publisherId, isUpdate, setIsUpdate]);

  const isMobile = isMobileCheck()
  return (
    <div className={classNames("w-full flex flex-col gap-2", {
      'max-w-[900px]': !isMobile,
      'p-2 h-screen w-screen': isMobile
    })}>
      {!isMobile && <HomeButton />}
      <SearchHeader
        hideSearch
        hidePublish
        onBack={showMetadataForm ? () => setShowMetadataForm(false) : undefined}
      />
      {isConfirming && (
        <MessagePopup
          type="info"
          content="Transaction submitted. Waiting for confirmation..."
          onClose={() => { }}
        />
      )}
      {isConfirmed && (
        <MessagePopup
          type="success"
          content={`Package ${isUpdate ? 'updated' : 'published'} successfully!`}
          onClose={() => { }}
        />
      )}
      {error && (
        <MessagePopup
          type="error"
          content={`Error: ${error.message}`}
          onClose={() => { }}
        />
      )}
      <div className="flex-center justify-between">
        <h4>Publish Package</h4>
        {Boolean(address) && <div className="card flex-center">
          <span>Publishing as:</span>
          <span className="font-mono">{address?.slice(0, 4)}...{address?.slice(-4)}</span>
        </div>}
      </div>

      {isConfirming ? (
        <Loader msg="Publishing package..." />
      ) : showMetadataForm ? (
        <MetadataForm {...{ packageName, publisherId, app: state?.app }} goBack={() => setShowMetadataForm(false)} />
      ) : !address || !isConnected ? (
        <>
          <h4>Please connect your wallet {isMobile && <br />} to publish a package</h4>
          <ConnectButton />
        </>
      ) : isConnecting ? (
        <Loader msg="Approve connection in your wallet" />
      ) : (
        <form
          className="flex flex-col flex-1 overflow-y-auto gap-2"
          onSubmit={publishPackage}
        >
          <div
            className="flex cursor-pointer p-2 -mb-2"
            onClick={() => setIsUpdate(!isUpdate)}
          >
            <Checkbox
              checked={isUpdate} readOnly
            />
            <label htmlFor="update" className="cursor-pointer ml-4">
              Update existing package
            </label>
          </div>
          <div className="flex flex-col">
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
          <div className="flex flex-col">
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
          <div className="flex flex-col gap-2">
            <label htmlFor="metadata-url">
              Metadata URL
            </label>
            <input
              id="metadata-url"
              type="text"
              required
              value={metadataUrl}
              onChange={(e) => setMetadataUrl(e.target.value)}
              onBlur={calculateMetadataHash}
              placeholder="https://github/my-org/my-repo/metadata.json"
            />
            <div>
              Metadata is a JSON file that describes your package.
              <br /> You can{" "}
              <a onClick={() => setShowMetadataForm(true)}
                className="underline cursor-pointer"
              >
                fill out a template here
              </a>
              .
            </div>
          </div>
          <div className="flex flex-col">
            <label htmlFor="metadata-hash">Metadata Hash</label>
            <input
              readOnly
              id="metadata-hash"
              type="text"
              value={metadataHash}
              onChange={(e) => setMetadataHash(e.target.value)}
              placeholder="Calculated automatically from metadata URL"
            />
          </div>
          <button type="submit" disabled={isConfirming}>
            {isConfirming ? 'Publishing...' : 'Publish'}
          </button>
        </form>
      )}

      <div className="flex flex-col">
        <h4>Packages You Own</h4>
        {myPublishedApps.length > 0 ? (
          <div className="flex flex-col">
            {myPublishedApps.map((app) => (
              <div key={`${app.package}${app.publisher}`} className="flex items-center justify-between">
                <div className="flex items-center">
                  <Jazzicon address={app.publisher} className="mr-2" />
                  <span>{app.package}</span>
                </div>
                <button className="flex items-center" onClick={() => unpublishPackage(app.package, app.publisher)}>
                  <span>Unpublish</span>
                </button>
              </div>
            ))}
          </div>
        ) : (
          <div className="flex items-center">
            <span>No packages published</span>
          </div>
        )}
      </div>
    </div>
  );
}
