use alloy_sol_macro::sol;

sol! {
    function mint (
        address to,
        bytes calldata label,
        bytes calldata initialization,
        address implementation
    ) external returns (
        address tba
    );

    function get (
        bytes32 namehash
    ) external view returns (
        address tba,
        address owner,
        bytes data,
    );

    function fact (
        bytes calldata fact,
        bytes calldata data
    ) external returns (
        bytes32 labelhash
    );

    function note (
        bytes calldata note,
        bytes calldata data
    ) external returns (
        bytes32 labelhash
    );

    // tba account
    function execute(
        address to,
        uint256 value,
        bytes calldata data,
        uint8 operation
    ) external payable returns (bytes memory returnData);

    /// The data structure signed by user to bootstrap the node.
    /// Never posted onchain or across network.
    struct Boot {
        string username;
        bytes32 password_hash;
        uint256 timestamp;
        bool direct;
        bool reset;
        uint256 chain_id;
    }

    struct Call {
        address target;
        bytes callData;
    }

    function aggregate(
        Call[] calldata calls
    ) external payable returns (uint256 blockNumber, bytes[] memory returnData);

    function token() external view returns (uint256,address,uint256);
}
