import type { AddEthereumChainParameter } from '@web3-react/types'
import { ChainId } from '../constants/chainId'

const ETH: AddEthereumChainParameter['nativeCurrency'] = {
  name: 'Ether',
  symbol: 'ETH',
  decimals: 18,
}

interface ChainInformation {
  urls: string[]
  name: string
  nativeCurrency: AddEthereumChainParameter['nativeCurrency']
  blockExplorerUrls: AddEthereumChainParameter['blockExplorerUrls']
}


export function getAddChainParameters(chainId: number): AddEthereumChainParameter | number {
  const chainInformation = CHAINS[chainId]
  return {
    chainId,
    chainName: chainInformation.name,
    nativeCurrency: chainInformation.nativeCurrency,
    rpcUrls: chainInformation.urls,
    blockExplorerUrls: chainInformation.blockExplorerUrls,
  }
}

type ChainConfig = { [chainId: number]: ChainInformation }

export const MAINNET_CHAINS: ChainConfig = {
  [ChainId.OPTIMISM]: {
    urls: [''].filter(Boolean), // TODO uhhhh
    name: 'Optimism',
    nativeCurrency: ETH,
    blockExplorerUrls: ['https://optimistic.etherscan.io'],
  },
}

export const TESTNET_CHAINS: ChainConfig = {
  [ChainId.OPTIMISM_GOERLI]: {
    urls: ['https://goerli.optimism.io'],
    name: 'Optimism Goerli',
    nativeCurrency: ETH,
    blockExplorerUrls: ['https://goerli-explorer.optimism.io'],
  },
  [ChainId.LOCAL]: {
    urls: ['http://localhost:8545'],
    name: 'Localhost 8545',
    nativeCurrency: ETH,
    blockExplorerUrls: [],
  }
}

export const CHAINS: ChainConfig = {
  ...MAINNET_CHAINS,
  ...TESTNET_CHAINS,
}
