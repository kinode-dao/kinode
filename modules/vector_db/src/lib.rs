use std::collections::HashMap;

use uqbar_process_lib::{Address, Response, set_state, print_to_terminal, get_typed_state, receive, Message, Payload};

use serde::{Deserialize, Serialize};
use serde_json::json;

use shinkai_vector_resources::data_tags::DataTag;
use shinkai_vector_resources::document_resource::DocumentVectorResource;
use shinkai_vector_resources::embeddings::Embedding;
use shinkai_vector_resources::source::VRSource;
use shinkai_vector_resources::vector_resource::VectorResource;
use shinkai_vector_resources::vector_resource_types::RetrievedNode;

wit_bindgen::generate!({
    path: "../../wit",
    world: "process",
    exports: {
        world: Component,
    },
});

struct Component;

#[derive(Serialize, Deserialize, Debug, Clone, Eq, Hash, PartialEq)]
struct DocProcessId {
    process_name: String,
    package_name: String,
    publisher_node: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct Query {
    process_id: DocProcessId,
    doc_id: String,
    embedding_id: String,
    vector: Vec<f32>,
    limit: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct CreateDoc {
    process_id: DocProcessId,
    name: String,
    description: Option<String>,
    source: VRSource,
    doc_id: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct NewEmbedding {
    id: String,
    text: String,
    vector: Vec<f32>,
    metadata: Option<HashMap<String, String>>,
    parsing_tags: Vec<DataTag>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct AppendEmbeddings {
    process_id: DocProcessId,
    doc_id: String,
    vectors: Vec<NewEmbedding>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
enum VectorRequest {
    CreateDoc(CreateDoc),
    Append(AppendEmbeddings),
    Query(Query),
}

#[derive(Serialize, Deserialize, Debug, Clone, Eq, Hash, PartialEq)]
struct DocId {
    process_id: DocProcessId,
    doc_id: String,
}

type DocumentDb = HashMap<DocId, DocumentVectorResource>; // Map of process_id to DocumentStore
type StoredDocumentDb = HashMap<DocId, String>; // Map of process_id to DocumentStore as JSON

#[derive(Serialize, Deserialize, Debug, Clone)]
struct VectorDbState {
    pub doc_db: DocumentDb,
}

// impl VectorDbState {
//     pub fn serialize(&self) -> StoredVectorDbState {
//         let mut stored_doc_db = HashMap::new();
//         for (doc_id, doc) in &self.doc_db {
//             stored_doc_db.insert(
//                 doc_id.clone(),
//                 serde_json::to_string(&doc).unwrap(),
//             );
//         }
//         StoredVectorDbState { doc_db: stored_doc_db }
//     }
// }

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredVectorDbState {
    pub doc_db: StoredDocumentDb,
}

fn send_success(doc_id: String) -> Result<(), anyhow::Error> {
    Response::new()
        .ipc(
            json!({
                "success": true,
                "doc_id": doc_id,
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        )
        .send()
}

fn send_failure(doc_id: String, message: String) -> Result<(), anyhow::Error> {
    Response::new()
        .ipc(
            json!({
                "success": false,
                "doc_id": doc_id,
                "message": message,
            })
            .to_string()
            .as_bytes()
            .to_vec(),
        )
        .send()
}

pub fn save_vector_db_state(state: &VectorDbState) {
    set_state(&bincode::serialize(&state).unwrap());
}

impl Guest for Component {
    fn init(our: String) {
        print_to_terminal(0, "vector_db: start");
        let our = Address::from_str(&our).unwrap();

        // let bindings_address = Address {
        //     node: our.node.clone(),
        //     process: DocProcessId::from_str("http_server:sys:uqbar").unwrap(),
        // };

        // let http_endpoint_binding_requests: [(Address, Request, Option<Context>, Option<Payload>); 1] = [
        //     generate_http_binding(bindings_address.clone(), "/", true),
        // ];
        // send_requests(&http_endpoint_binding_requests);

        let mut state: VectorDbState = match get_typed_state(|bytes| {
            Ok(bincode::deserialize::<VectorDbState>(bytes)?)
        }) {
            Some(state) => state,
            None => VectorDbState { doc_db: HashMap::new() }
        };

        loop {
            let Ok((_source, message)) = receive() else {
                print_to_terminal(1, "vector_db: got network error");
                continue;
            };
            let Message::Request(request) = message else {
                print_to_terminal(
                    1,
                    &format!("vector_db: got unexpected message: {:?}", message),
                );
                continue;
            };

            match serde_json::from_slice::<VectorRequest>(&request.ipc) {
                Ok(request) => match request {
                    VectorRequest::CreateDoc(create) => {
                        let process = create.process_id;
                        let doc = DocumentVectorResource::new_empty(
                            &create.name,
                            create.description.as_deref(),
                            create.source,
                            &create.doc_id,
                        );

                        state.doc_db.insert(
                            DocId {
                                process_id: process.clone(),
                                doc_id: create.doc_id.clone(),
                            },
                            doc,
                        );

                        save_vector_db_state(&state);
                        match send_success(create.doc_id) {
                            Ok(_) => {}
                            Err(err) => {
                                print_to_terminal(
                                    1,
                                    &format!("vector_db: failed to send success response: {}", err),
                                );
                            }
                        };
                    }
                    VectorRequest::Append(append) => {
                        let process = append.process_id;

                        match state.doc_db.get_mut(&DocId {
                            process_id: process.clone(),
                            doc_id: append.doc_id.clone(),
                        }) {
                            Some(doc) => {
                                for vector in append.vectors {
                                    doc.append_text_node(
                                        &vector.text,
                                        vector.metadata,
                                        &Embedding {
                                            id: vector.id.clone(),
                                            vector: vector.vector.clone(),
                                        },
                                        &vector.parsing_tags,
                                    );
                                }
                            }
                            None => {
                                print_to_terminal(
                                    1,
                                    &format!("vector_db: failed to find doc: {}", append.doc_id),
                                );

                                let mut doc = DocumentVectorResource::new_empty(
                                    &append.doc_id.clone(),
                                    None,
                                    VRSource::None,
                                    &append.doc_id,
                                );

                                for vector in append.vectors {
                                    doc.append_text_node(
                                        &vector.text,
                                        vector.metadata,
                                        &Embedding {
                                            id: vector.id.clone(),
                                            vector: vector.vector.clone(),
                                        },
                                        &vector.parsing_tags,
                                    );
                                }

                                state.doc_db.insert(
                                    DocId {
                                        process_id: process.clone(),
                                        doc_id: append.doc_id.clone(),
                                    },
                                    doc,
                                );

                            }
                        };

                        save_vector_db_state(&state);
                        match send_success(append.doc_id) {
                            Ok(_) => {
                                print_to_terminal(1, "vector_db: embed append success");
                            }
                            Err(err) => {
                                print_to_terminal(
                                    1,
                                    &format!("vector_db: failed to send success response: {}", err),
                                );
                            }
                        };
                    }
                    VectorRequest::Query(query) => {
                        let process = query.process_id;
                        let doc = match state.doc_db.get_mut(&DocId {
                            process_id: process.clone(),
                            doc_id: query.doc_id.clone(),
                        }) {
                            Some(doc) => doc,
                            None => {
                                print_to_terminal(
                                    1,
                                    &format!("vector_db: failed to find doc: {}", query.doc_id),
                                );
                                match send_failure(
                                    query.doc_id,
                                    format!("failed to find doc for query"),
                                ) {
                                    Ok(_) => {}
                                    Err(err) => {
                                        print_to_terminal(
                                            1,
                                            &format!(
                                                "vector_db: failed to send failure response: {}",
                                                err
                                            ),
                                        );
                                    }
                                }
                                continue;
                            }
                        };

                        let query_embedding = Embedding {
                            id: query.embedding_id,
                            vector: query.vector,
                        };
                        let res: Vec<RetrievedNode> =
                            doc.vector_search(query_embedding.clone(), query.limit);
                        let res_bytes = json!(res).to_string().as_bytes().to_vec();

                        match Response::new()
                            .ipc(Vec::new())
                            .payload(Payload {
                                mime: Some("application/json".to_string()),
                                bytes: res_bytes,
                            })
                            .send() {
                                Ok(_) => {}
                                Err(err) => {
                                    print_to_terminal(
                                        1,
                                        &format!("vector_db: failed to send response: {}", err),
                                    );
                                }
                            }
                    }
                },
                Err(err) => {
                    print_to_terminal(1, &format!("vector_db: failed to parse JSON: {}", err));
                }
            }
        }
    }
}
