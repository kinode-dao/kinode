import { ethers } from "ethers";
import { PackageStore } from "../abis/types";

export interface PageProps {
  provider?: ethers.providers.Web3Provider;
  packageAbi?: PackageStore
}
