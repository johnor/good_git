use anyhow::{anyhow, Context, Result};
use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};
use std::io::prelude::*;

#[derive(Debug)]
pub struct Blob {
    pub content: Vec<u8>,
}

impl Blob {
    pub fn new(content: Vec<u8>) -> Blob {
        Blob { content }
    }

    pub fn hash(self) -> String {
        let size = self.content.len();
        let data = format!("blob {size}\0");
        let mut data = data.as_bytes().to_vec();
        data.extend(self.content);

        hash(&data)
    }
}

#[derive(Debug)]
pub struct Tree {
    pub files: Vec<File>,
}

impl Tree {
    pub fn new(files: Vec<File>) -> Tree {
        Tree { files }
    }
}

#[derive(Debug, PartialEq)]
pub struct File {
    pub mode: String,
    pub name: String,
    pub hash: String,
}

impl File {
    pub fn type_str(&self) -> &str {
        // Possible values:
        // 100644: normal file (blob)
        // 100755: executable file (blob)
        // 120000: symbolic link
        // 40000: tree
        // 160000: submodule
        match self.mode.as_str() {
            "100644" => "blob",
            "100755" => "blob",
            "120000" => "symlink",
            "40000" => "tree",
            "160000" => "submodule",
            _ => "unknown",
        }
    }
}

#[derive(Debug)]
pub enum Object {
    Blob(Blob),
    Tree(Tree),
}

impl Object {
    pub fn from_bytes(s: &[u8]) -> Result<Object> {
        let (object_type, object_size, header_end) = Object::parse_header(s)?;
        let mut content = &s[header_end + 1..];

        if content.len() != object_size {
            return Err(anyhow!("Incorrect header length"));
        }

        match object_type.as_str() {
            "blob" => {
                let blob = Blob::new(content.to_vec());
                Ok(Object::Blob(blob))
            }
            "tree" => {
                // Format (one per file/folder/tree/submodule):
                // [mode] [object name]\0[SHA-1 in binary format (20 bytes)]
                let mut files = vec![];
                while !content.is_empty() {
                    let mut mode = vec![];
                    let mode_size = content
                        .read_until(b' ', &mut mode)
                        .context("Failed to read mode")?;
                    let mode = std::str::from_utf8(&mode[..mode_size - 1])?;

                    let mut name = vec![];
                    let name_size = content
                        .read_until(b'\0', &mut name)
                        .context("Failed to read file name")?;
                    let name = std::str::from_utf8(&name[..name_size - 1])?;

                    let mut hash = [0_u8; 20];
                    content
                        .read_exact(&mut hash)
                        .context("Failed to read hash")?;
                    let hash = hex::encode(hash);

                    files.push(File {
                        mode: mode.to_string(),
                        name: name.to_string(),
                        hash,
                    });
                }
                let tree = Tree::new(files);
                Ok(Object::Tree(tree))
            }
            _ => Err(anyhow!("Unknown object type")),
        }
    }

    pub fn from_file(path: &std::path::Path) -> Result<Object> {
        let data = std::fs::read(path).context("Could not read from file")?;
        let mut z = ZlibDecoder::new(&data[..]);
        let mut s: Vec<u8> = vec![];
        z.read_to_end(&mut s)?;

        Object::from_bytes(&s)
    }

    /// Parse the header of a git object.
    ///
    /// The header is in the format: [object type] [object size]\0
    ///
    /// Returns the type, object size and the index where the header ends.
    fn parse_header(s: &[u8]) -> Result<(String, usize, usize)> {
        let space_index = s
            .iter()
            .position(|&x| x == b' ')
            .ok_or(anyhow!("Incorrect header format"))?;
        let null_index = s
            .iter()
            .position(|&x| x == b'\0')
            .ok_or(anyhow!("Incorrect header format"))?;
        let object_type = std::str::from_utf8(&s[..space_index])?;
        let object_size = std::str::from_utf8(&s[space_index + 1..null_index])?;
        let object_size = object_size.parse::<usize>()?;
        Ok((object_type.to_string(), object_size, null_index))
    }
}

pub fn hash(s: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(s);

    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use crate::object::File;

    use super::hash;
    use super::Blob;
    use super::Object;
    #[test]
    fn test_object_parse_header() {
        assert_eq!(
            Object::parse_header(b"blob 16\0").unwrap(),
            ("blob".to_string(), 16, 7)
        );
    }

    #[test]
    fn test_object_parse_header_incorrect_format() {
        assert_eq!(
            Object::parse_header(b"blob 16").unwrap_err().to_string(),
            "Incorrect header format"
        );
        assert_eq!(
            Object::parse_header(b"blob").unwrap_err().to_string(),
            "Incorrect header format"
        );
    }

    #[test]
    fn test_object_from_bytes_for_blob() {
        let s = b"blob 16\0what is up, doc?";
        let object = Object::from_bytes(s.as_ref()).unwrap();
        let Object::Blob(blob) = object else {
            panic!("Expected a Blob");
        };
        assert_eq!(blob.content, b"what is up, doc?");
    }

    #[test]
    fn test_object_from_bytes_for_tree() {
        let s = b"tree 107\0\
            100644 file1.txt\0\x01\x02\x03\x04\x05\x06\x07\x08\x09\x0a\x0b\x0c\x0d\x0e\x0f\x10\x11\x12\x13\x14\
            100644 file2.txt\0\x51\x52\x53\x54\x55\x56\x57\x58\x59\x5a\x5b\x5c\x5d\x5e\x5f\x60\x61\x62\x63\x64\
            40000 folder\0\x81\x82\x83\x84\x85\x86\x87\x88\x89\x8a\x8b\x8c\x8d\x8e\x8f\x90\x91\x92\x93\x94";
        let object = Object::from_bytes(s.as_ref()).unwrap();
        let Object::Tree(tree) = object else {
            panic!("Expected a tree");
        };
        assert_eq!(
            tree.files,
            vec![
                File {
                    mode: "100644".to_string(),
                    name: "file1.txt".to_string(),
                    hash: "0102030405060708090a0b0c0d0e0f1011121314".to_string(),
                },
                File {
                    mode: "100644".to_string(),
                    name: "file2.txt".to_string(),
                    hash: "5152535455565758595a5b5c5d5e5f6061626364".to_string(),
                },
                File {
                    mode: "40000".to_string(),
                    name: "folder".to_string(),
                    hash: "8182838485868788898a8b8c8d8e8f9091929394".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_object_from_bytes_for_tree_incorrect_hash_length() {
        let s = b"tree 18\0\
            100644 file1.txt\0\x01";
        let err = Object::from_bytes(s.as_ref()).unwrap_err().to_string();
        assert_eq!(err, "Failed to read hash");
    }

    #[test]
    fn test_object_from_bytes_incorrect_header_size() {
        let s = b"blob 0\0hi";
        let err = Object::from_bytes(s.as_ref()).unwrap_err().to_string();
        assert_eq!(err, "Incorrect header length");
    }

    #[test]
    fn test_blob_hash_is_correct() {
        // From https://git-scm.com/book/sv/v2/Git-Internals-Git-Objects
        let blob = Blob::new(b"what is up, doc?".to_vec());
        assert_eq!(blob.hash(), "bd9dbf5aae1a3862dd1526723246b20206e5fc37");
    }

    #[test]
    fn test_hash_is_correct() {
        // From https://git-scm.com/book/sv/v2/Git-Internals-Git-Objects
        let s = b"blob 16\0what is up, doc?";
        assert_eq!(hash(s), "bd9dbf5aae1a3862dd1526723246b20206e5fc37");
    }
}
