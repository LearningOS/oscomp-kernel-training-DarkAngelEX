// pub mod common;
// pub mod directory;
// pub mod file;

use alloc::{collections::LinkedList, string::String};


pub enum Access {
    ReadOnly,
    Writable,
}

pub enum FileTy {
    File,
    Directory,
}

pub struct AccessPath {
    pub path: LinkedList<String>, // 指向一个存在的目录 如果为空则为根目录
    pub name: String,             // 要操作的对象名
    pub access: Access,           // 访问标志
    pub file_ty: Option<FileTy>,  // 文件类型
}

impl AccessPath {
    pub const fn new() -> Self {
        Self {
            path: LinkedList::new(),
            name: String::new(),
            access: Access::ReadOnly,
            file_ty: None,
        }
    }
    pub fn path_add(&mut self, name: String) -> &mut Self {
        debug_assert!(!["", ".", ".."].contains(&name.as_str()));
        self.path.push_back(name);
        self
    }
    pub fn set_name(&mut self, name: String) -> &mut Self {
        debug_assert!(!name.is_empty());
        self.name = name;
        self
    }
    pub fn set_access(&mut self, access: Access) -> &mut Self {
        self.access = access;
        self
    }
    pub fn set_file_ty(&mut self, file_ty: FileTy) -> &mut Self {
        self.file_ty = Some(file_ty);
        self
    }
    pub fn assert_access(&self) {
        assert!(self.name.is_empty());
        assert!(matches!(self.file_ty, None));
    }
    pub fn assert_create(&self) {
        assert!(!self.name.is_empty());
        assert!(matches!(self.file_ty, Some(_)));
    }
    pub fn assert_delete(&self) {
        assert!(!self.name.is_empty());
        assert!(matches!(self.file_ty, Some(_)));
    }
    pub fn is_unknown_file_ty(&self) -> bool {
        self.file_ty.is_none()
    }
}
