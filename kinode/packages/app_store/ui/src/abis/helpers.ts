import { multicallAbi, kinomapAbi, mechAbi, KINOMAP, MULTICALL, KINO_ACCOUNT_IMPL } from "./";
import { encodeFunctionData, encodePacked, stringToHex } from "viem";

export function encodeMulticalls(metadataUri: string, metadataHash: string) {
    const metadataUriCall = encodeFunctionData({
        abi: kinomapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex("~metadata-uri")]),
            encodePacked(["bytes"], [stringToHex(metadataUri)]),
        ]
    })

    const metadataHashCall = encodeFunctionData({
        abi: kinomapAbi,
        functionName: 'note',
        args: [
            encodePacked(["bytes"], [stringToHex("~metadata-hash")]),
            encodePacked(["bytes"], [stringToHex(metadataHash)]),
        ]
    })

    const calls = [
        { target: KINOMAP, callData: metadataUriCall },
        { target: KINOMAP, callData: metadataHashCall }
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
            BigInt(0), // value
            multicalls,
            1
        ]
    });

    const mintCall = encodeFunctionData({
        abi: kinomapAbi,
        functionName: 'mint',
        args: [
            our_address,
            encodePacked(["bytes"], [stringToHex(app_name)]),
            initCall,
            "0x", // erc721 details? <- encode app_store here? actually might be a slick way to do it. 
            KINO_ACCOUNT_IMPL,
        ]
    })
    return mintCall;
}