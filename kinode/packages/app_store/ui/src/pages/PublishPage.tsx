import React, { useState, useCallback, FormEvent, useEffect } from "react";
import { useLocation } from "react-router-dom";
import { BigNumber, utils } from "ethers";
import { useWeb3React } from "@web3-react/core";

import SearchHeader from "../components/SearchHeader";
import { PageProps } from "../types/Page";
import { setChain } from "../utils/chain";
import { OPTIMISM_OPT_HEX } from "../constants/chain";
import { hooks, metaMask } from "../utils/metamask";
import Loader from "../components/Loader";
import { toDNSWireFormat } from "../utils/dnsWire";
import useAppsStore from "../store/apps-store";
import MetadataForm from "../components/MetadataForm";
import { AppInfo } from "../types/Apps";
import Checkbox from "../components/Checkbox";
import Jazzicon from "../components/Jazzicon";
import { Tooltip } from "../components/Tooltip";
import HomeButton from "../components/HomeButton";
import classNames from "classnames";
import { isMobileCheck } from "../utils/dimensions";

const { useIsActivating } = hooks;

interface PublishPageProps extends PageProps { }

export default function PublishPage({
  provider,
  packageAbi,
}: PublishPageProps) {
  // get state from router
  const { state } = useLocation();
  const { listedApps } = useAppsStore();
  // TODO: figure out how to handle provider
  const { account, isActive } = useWeb3React();
  const isActivating = useIsActivating();

  const [loading, setLoading] = useState("");
  const [publishSuccess, setPublishSuccess] = useState<
    { packageName: string; publisherId: string } | undefined
  >();
  const [showMetadataForm, setShowMetadataForm] = useState<boolean>(false);
  const [packageName, setPackageName] = useState<string>("");
  const [publisherId, setPublisherId] = useState<string>(
    window.our?.node || ""
  ); // BytesLike
  const [metadataUrl, setMetadataUrl] = useState<string>("");
  const [metadataHash, setMetadataHash] = useState<string>(""); // BytesLike
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
      listedApps.filter((app) => app.owner?.toLowerCase() === account?.toLowerCase())
    );
  }, [listedApps, account])

  const connectWallet = useCallback(async () => {
    await metaMask.activate().catch(() => { });

    try {
      setChain(OPTIMISM_OPT_HEX);
    } catch (error) {
      console.error(error);
    }
  }, []);

  const calculateMetadataHash = useCallback(async () => {
    if (!metadataUrl) {
      setMetadataHash("");
      return;
    }
    try {
      const metadataResponse = await fetch(metadataUrl);
      const metadataText = await metadataResponse.text();
      JSON.parse(metadataText); // confirm it's valid JSON
      const metadataHash = utils.keccak256(utils.toUtf8Bytes(metadataText));
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

      let metadata = metadataHash;

      try {
        if (!metadata) {
          // https://pongo-uploads.s3.us-east-2.amazonaws.com/chat_metadata.json
          const metadataResponse = await fetch(metadataUrl);
          await metadataResponse.json(); // confirm it's valid JSON
          const metadataText = await metadataResponse.text(); // hash as text
          metadata = utils.keccak256(utils.toUtf8Bytes(metadataText));
        }

        setLoading("Please confirm the transaction in your wallet");
        const publisherIdDnsWireFormat = toDNSWireFormat(publisherId);
        await setChain(OPTIMISM_OPT_HEX);

        // TODO: have a checkbox to show if it's an update of an existing package

        const tx = await (isUpdate
          ? packageAbi?.updateMetadata(
            BigNumber.from(
              utils.solidityKeccak256(
                ["string", "bytes"],
                [packageName, publisherIdDnsWireFormat]
              )
            ),
            metadataUrl,
            metadata
          )
          : packageAbi?.registerApp(
            packageName,
            publisherIdDnsWireFormat,
            metadataUrl,
            metadata
          ));

        await new Promise((resolve) => setTimeout(resolve, 2000));

        setLoading("Publishing package...");
        await tx?.wait();
        setPublishSuccess({ packageName, publisherId });
        setPackageName("");
        setPublisherId(window.our?.node || publisherId);
        setMetadataUrl("");
        setMetadataHash("");
        setIsUpdate(false);
      } catch (error) {
        console.error(error);
        window.alert(
          "Error publishing package. Please ensure the package name and publisher ID are valid, and the metadata is in JSON format."
        );
      } finally {
        setLoading("");
      }
    },
    [
      packageName,
      isUpdate,
      publisherId,
      metadataUrl,
      metadataHash,
      packageAbi,
      setPublishSuccess,
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
        await setChain(OPTIMISM_OPT_HEX);

        const tx = await
          packageAbi?.unlistPacakge(
            utils.keccak256(utils.solidityPack(
              ["string", "bytes"],
              [packageName, toDNSWireFormat(publisherName)]
            ))
          );

        await new Promise((resolve) => setTimeout(resolve, 2000));

        setLoading("Unlisting package...");
        await tx?.wait();
      } catch (error) {
        console.error(error);
        window.alert(
          "Error unlisting package"
        );
      } finally {
        setLoading("");
      }
    },
    [packageAbi, setLoading]
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
      <div className="flex-center justify-between">
        <h4>Publish Package</h4>
        {Boolean(account) && <div className="card flex-center">
          <span>Publishing as:</span>
          <Jazzicon address={account!} className="mx-2" />
          <span className="font-mono">{account?.slice(0, 4)}...{account?.slice(-4)}</span>
        </div>}
      </div>

      {loading ? (
        <div className="flex-col-center">
          <Loader msg={loading} />
        </div>
      ) : publishSuccess ? (
        <div className="flex-col-center gap-2">
          <h4>Package Published!</h4>
          <div>
            <strong>Package Name:</strong> {publishSuccess.packageName}
          </div>
          <div>
            <strong>Publisher ID:</strong> {publishSuccess.publisherId}
          </div>
          <button
            className={`flex ml-2`}
            onClick={() => setPublishSuccess(undefined)}
          >
            Publish Another Package
          </button>
        </div>
      ) : showMetadataForm ? (
        <MetadataForm {...{ packageName, publisherId, app: state?.app }} goBack={() => setShowMetadataForm(false)} />
      ) : !account || !isActive ? (
        <>
          <h4>Please connect your wallet {isMobile && <br />} to publish a package</h4>
          <button className={`connect-wallet row`} onClick={connectWallet}>
            Connect Wallet
          </button>
        </>
      ) : isActivating ? (
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
          <button type="submit">
            Publish
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
