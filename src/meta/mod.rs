mod date;
mod filetype;
mod indicator;
mod inode;
mod name;
mod owner;
mod permissions;
mod size;
mod symlink;

#[cfg(windows)]
mod windows_utils;

pub use self::date::Date;
pub use self::filetype::FileType;
pub use self::indicator::Indicator;
pub use self::inode::INode;
pub use self::name::Name;
pub use self::owner::Owner;
pub use self::permissions::Permissions;
pub use self::size::Size;
pub use self::symlink::SymLink;
pub use crate::flags::Display;
pub use crate::icon::Icons;
use crate::print_error;

use std::io::{Error, ErrorKind};
use std::path::PathBuf;

use globset::GlobSet;

use futures::future::{self, BoxFuture, Future, FutureExt, TryFutureExt};
use futures::stream::StreamExt;
use tokio::fs::{canonicalize, metadata, read_dir, read_link, symlink_metadata, DirEntry};

#[derive(Clone, Debug)]
pub struct Meta {
    pub name: Name,
    pub path: PathBuf,
    pub permissions: Permissions,
    pub date: Date,
    pub owner: Owner,
    pub file_type: FileType,
    pub size: Size,
    pub symlink: SymLink,
    pub indicator: Indicator,
    pub inode: INode,
    pub content: Option<Vec<Meta>>,
}

impl Meta {
    pub fn recurse_into<'a: 'b, 'b>(
        &'a self,
        depth: usize,
        display: Display,
        ignore_globs: &'b GlobSet,
    ) -> BoxFuture<'b, Result<Option<Vec<Meta>>, std::io::Error>> {
        if depth == 0 {
            return future::ready(Ok(None)).boxed();
        }

        if display == Display::DisplayDirectoryItself {
            return future::ready(Ok(None)).boxed();
        }

        match self.file_type {
            FileType::Directory { .. } => (),
            _ => return future::ready(Ok(None)).boxed(),
        }

        async move {
            let entries = match read_dir(&self.path).await {
                Ok(entries) => entries,
                Err(err) => {
                    print_error!("cannot access '{}': {}", self.path.display(), err);
                    return Ok(None);
                }
            };
            let mut content: Vec<Meta> = Vec::new();

            if let Display::DisplayAll = display {
                let mut current_meta;
                let mut parent_meta;

                let absolute_path = canonicalize(&self.path).await?;
                let parent_path = match absolute_path.parent() {
                    None => PathBuf::from("/"),
                    Some(path) => PathBuf::from(path),
                };

                current_meta = self.clone();
                current_meta.name.name = ".".to_string();

                parent_meta = Self::from_path(&parent_path).await?;
                parent_meta.name.name = "..".to_string();

                content.push(current_meta);
                content.push(parent_meta);
            }
            for entry in entries
                .collect::<Vec<Result<DirEntry, std::io::Error>>>()
                .await
            {
                let path = entry?.path();

                let name = path
                    .file_name()
                    .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "invalid file name"))?;

                if ignore_globs.is_match(&name) {
                    continue;
                }

                if let Display::DisplayOnlyVisible = display {
                    if name.to_string_lossy().starts_with('.') {
                        continue;
                    }
                }

                let mut entry_meta = match Self::from_path(&path).await {
                    Ok(res) => res,
                    Err(err) => {
                        print_error!("cannot access '{}': {}", path.display(), err);
                        continue;
                    }
                };
                match entry_meta
                    .recurse_into(depth - 1, display, ignore_globs)
                    .await
                {
                    Ok(content) => entry_meta.content = content,
                    Err(err) => {
                        print_error!("cannot access '{}': {}", path.display(), err);
                        continue;
                    }
                };

                content.push(entry_meta);
            }
            Ok(Some(content))
        }
        .boxed()
    }

    pub async fn calculate_total_size(&mut self) {
        if let FileType::Directory { .. } = self.file_type {
            if let Some(metas) = &mut self.content {
                let mut size_accumulated = self.size.get_bytes();
                for x in &mut metas.iter_mut() {
                    x.calculate_total_size();
                    size_accumulated += x.size.get_bytes();
                }
                self.size = Size::new(size_accumulated);
            } else {
                // possibility that 'depth' limited the recursion in 'recurse_into'
                self.size = Size::new(Meta::calculate_total_file_size(&self.path).await);
            }
        }
    }

    fn calculate_total_file_size<'a>(path: &'a PathBuf) -> BoxFuture<'a, u64> {
        async move {
            let metadata = read_link(path).then(|sym_path| Self::read_metadata(path, sym_path));
            let metadata = match metadata.await {
                Ok(meta) => meta,
                Err(err) => {
                    print_error!("cannot access '{}': {}", path.display(), err);
                    return 0;
                }
            };
            let file_type = metadata.file_type();
            if file_type.is_file() {
                metadata.len()
            } else if file_type.is_dir() {
                let mut size = metadata.len();

                let entries = match path.read_dir() {
                    Ok(entries) => entries,
                    Err(err) => {
                        print_error!("cannot access '{}': {}", path.display(), err);
                        return size;
                    }
                };
                for entry in entries {
                    let path = match entry {
                        Ok(entry) => entry.path(),
                        Err(err) => {
                            print_error!("cannot access '{}': {}", path.display(), err);
                            continue;
                        }
                    };
                    size += Meta::calculate_total_file_size(&path).await;
                }
                size
            } else {
                0
            }
        }
        .boxed()
    }

    pub fn from_path<'a>(
        path: &'a PathBuf,
    ) -> impl Future<Output = Result<Self, std::io::Error>> + 'a {
        read_link(path)
            .then(move |sym_path| Self::read_metadata(path, sym_path))
            .map_ok(move |metadata| Self::from_metadata(path, metadata))
    }

    fn read_metadata<'a>(
        path: &'a PathBuf,
        link: Result<PathBuf, std::io::Error>,
    ) -> impl Future<Output = Result<std::fs::Metadata, std::io::Error>> + 'a {
        if link.is_ok() {
            symlink_metadata(path).left_future()
        } else {
            metadata(path).right_future()
        }
    }

    fn from_metadata(path: &PathBuf, metadata: std::fs::Metadata) -> Self {
        #[cfg(unix)]
        let owner = Owner::from(&metadata);
        #[cfg(unix)]
        let permissions = Permissions::from(&metadata);

        #[cfg(windows)]
        let (owner, permissions) = windows_utils::get_file_data(&path)?;

        let file_type = FileType::new(&metadata, &permissions);
        let name = Name::new(&path, file_type);
        let inode = INode::from(&metadata);

        Self {
            inode,
            path: path.to_path_buf(),
            symlink: SymLink::from(path.as_path()),
            size: Size::from(&metadata),
            date: Date::from(&metadata),
            indicator: Indicator::from(file_type),
            owner,
            permissions,
            name,
            file_type,
            content: None,
        }
    }
}
