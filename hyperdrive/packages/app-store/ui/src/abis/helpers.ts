import { multicallAbi, hypermapAbi, mechAbi, HYPERMAP, MULTICALL, KINO_ACCOUNT_UPGRADABLE_IMPL } from "./";
import { encodeFunctionData, encodePacked, stringToHex } from "viem";

export function encodeMulticalls(metadataUri: string, metadataHash: string) {
    const metadataHashCall = encodeFunctionData({
        abi: hypermapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex("~metadata-hash")]),
            encodePacked(["bytes"], [stringToHex(metadataHash)]),
        ]
    })

    const metadataUriCall = encodeFunctionData({
        abi: hypermapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex("~metadata-uri")]),
            encodePacked(["bytes"], [stringToHex(metadataUri)]),
        ]
    })

    const calls = [
        { target: HYPERMAP, callData: metadataHashCall },
        { target: HYPERMAP, callData: metadataUriCall },
    ];

    const multicall = encodeFunctionData({
        abi: multicallAbi,
        functionName: 'aggregate',
        args: [calls]
    });
    return multicall;
}

export function encodeIntoMintCall(multicalls: `0x${string}`, our_address: `0x${string}`, app_name: string) {
    const initCall = encodeFunctionData({
        abi: mechAbi,
        functionName: 'execute',
        args: [
            MULTICALL,
            BigInt(0),
            multicalls,
            1
        ]
    });

    const mintCall = encodeFunctionData({
        abi: hypermapAbi,
        functionName: 'mint',
        args: [
            our_address,
            encodePacked(["bytes"], [stringToHex(app_name)]),
            initCall,
            KINO_ACCOUNT_UPGRADABLE_IMPL,
        ]
    })
    return mintCall;
}
