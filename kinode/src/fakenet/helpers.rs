use alloy_sol_macro::sol;
use sha3::{Digest, Keccak256};

sol! {
    #[sol(rpc)]
    contract RegisterHelpers {
        function register(
            bytes calldata _name,
            address _to,
            bytes[] calldata _data
        ) external payable returns (uint256 nodeId_);

        function setKey(bytes32 _node, bytes32 _key);

        function setAllIp(
            bytes32 _node,
            uint128 _ip,
            uint16 _ws,
            uint16 _wt,
            uint16 _tcp,
            uint16 _udp
        );

        function ip(bytes32) external view returns (uint128, uint16, uint16, uint16, uint16);


        function ownerOf(uint256 node) returns (address);

        function multicall(bytes[] calldata data);
    }
}

pub fn dns_encode_fqdn(name: &str) -> Vec<u8> {
    let bytes_name = name.as_bytes();
    let mut dns_name = Vec::new();

    let mut last_pos = 0;
    for (i, &b) in bytes_name.iter().enumerate() {
        if b == b'.' {
            if i != last_pos {
                dns_name.push((i - last_pos) as u8); // length of the label
                dns_name.extend_from_slice(&bytes_name[last_pos..i]);
            }
            last_pos = i + 1;
        }
    }

    if last_pos < bytes_name.len() {
        dns_name.push((bytes_name.len() - last_pos) as u8);
        dns_name.extend_from_slice(&bytes_name[last_pos..]);
    }

    dns_name.push(0);

    dns_name
}

pub fn encode_namehash(name: &str) -> [u8; 32] {
    let mut node = [0u8; 32];
    if name.is_empty() {
        return node;
    }
    let mut labels: Vec<&str> = name.split('.').collect();
    labels.reverse();

    for label in labels.iter() {
        let mut hasher = Keccak256::new();
        hasher.update(label.as_bytes());
        let labelhash = hasher.finalize();
        hasher = Keccak256::new();
        hasher.update(&node);
        hasher.update(labelhash);
        node = hasher.finalize().into();
    }
    node
}

#[cfg(test)]
mod test {
    use super::encode_namehash;

    #[test]
    fn test_namehash() {
        // Test cases, same than used @ EIP137 `https://github.com/ethereum/EIPs/blob/master/EIPS/eip-137.md`
        let cases = vec![
            (
                "",
                &[
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                ],
            ),
            (
                "eth",
                &[
                    0x93, 0xcd, 0xeb, 0x70, 0x8b, 0x75, 0x45, 0xdc, 0x66, 0x8e, 0xb9, 0x28, 0x1,
                    0x76, 0x16, 0x9d, 0x1c, 0x33, 0xcf, 0xd8, 0xed, 0x6f, 0x4, 0x69, 0xa, 0xb,
                    0xcc, 0x88, 0xa9, 0x3f, 0xc4, 0xae,
                ],
            ),
            (
                "foo.eth",
                &[
                    0xde, 0x9b, 0x9, 0xfd, 0x7c, 0x5f, 0x90, 0x1e, 0x23, 0xa3, 0xf1, 0x9f, 0xec,
                    0xc5, 0x48, 0x28, 0xe9, 0xc8, 0x48, 0x53, 0x98, 0x1, 0xe8, 0x65, 0x91, 0xbd,
                    0x98, 0x1, 0xb0, 0x19, 0xf8, 0x4f,
                ],
            ),
        ];

        for (name, expected_namehash) in cases {
            let namehash: &[u8] = &encode_namehash(name);
            assert_eq!(namehash, expected_namehash);
        }
    }
}
