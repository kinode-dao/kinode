import { multicallAbi, kimapAbi, mechAbi, KIMAP, MULTICALL, KINO_ACCOUNT_IMPL } from "./";
import { encodeFunctionData, encodePacked, stringToHex } from "viem";

export function encodeMulticalls(metadataUri: string, metadataHash: string) {
    const metadataHashCall = encodeFunctionData({
        abi: kimapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex("~metadata-hash")]),
            encodePacked(["bytes"], [stringToHex(metadataHash)]),
        ]
    })

    const metadataUriCall = encodeFunctionData({
        abi: kimapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex("~metadata-uri")]),
            encodePacked(["bytes"], [stringToHex(metadataUri)]),
        ]
    })

    const calls = [
        { target: KIMAP, callData: metadataHashCall },
        { target: KIMAP, callData: metadataUriCall },
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
        abi: kimapAbi,
        functionName: 'mint',
        args: [
            our_address,
            encodePacked(["bytes"], [stringToHex(app_name)]),
            initCall,
            "0x",
            KINO_ACCOUNT_IMPL,
        ]
    })
    return mintCall;
}