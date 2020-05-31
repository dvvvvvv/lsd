use crate::color::{ColoredString, Colors, Elem};
#[cfg(unix)]
use std::collections::BTreeMap;
#[cfg(unix)]
use std::fs::Metadata;

#[derive(Clone, Debug)]
pub struct Owner {
    user: String,
    group: String,
}

impl Owner {
    #[cfg_attr(unix, allow(dead_code))]
    pub fn new(user: String, group: String) -> Self {
        Self { user, group }
    }
}

#[cfg(unix)]
impl<'a> From<&'a Metadata> for Owner {
    fn from(meta: &Metadata) -> Self {
        use std::os::unix::fs::MetadataExt;
        use users::{get_group_by_gid, get_user_by_uid};

        let user = match get_user_by_uid(meta.uid()) {
            Some(res) => res.name().to_string_lossy().to_string(),
            None => meta.uid().to_string(),
        };

        let group = match get_group_by_gid(meta.gid()) {
            Some(res) => res.name().to_string_lossy().to_string(),
            None => meta.gid().to_string(),
        };

        Self { user, group }
    }
}

impl Owner {
    pub fn render_user(&self, colors: &Colors) -> ColoredString {
        colors.colorize(self.user.clone(), &Elem::User)
    }

    pub fn render_group(&self, colors: &Colors) -> ColoredString {
        colors.colorize(self.group.clone(), &Elem::Group)
    }
}

#[cfg(unix)]
#[derive(Default)]
pub struct OwnerCache {
    user_cache: BTreeMap<u32, String>,
    group_cache: BTreeMap<u32, String>,
}

#[cfg(unix)]
impl OwnerCache {
    pub fn get_by_uid_and_gid(&mut self, uid: u32, gid: u32) -> Owner {
        Owner {
            user: self.get_user(uid).clone(),
            group: self.get_group(gid).clone(),
        }
    }

    pub fn get_by_metadata(&mut self, meta: &Metadata) -> Owner {
        use std::os::unix::fs::MetadataExt;
        self.get_by_uid_and_gid(meta.uid(), meta.gid())
    }

    fn get_user(&mut self, uid: u32) -> &mut String {
        use users::get_user_by_uid;
        self.user_cache
            .entry(uid)
            .or_insert_with(|| match get_user_by_uid(uid) {
                Some(res) => res.name().to_string_lossy().to_string(),
                None => uid.to_string(),
            })
    }

    fn get_group(&mut self, gid: u32) -> &mut String {
        use users::get_group_by_gid;
        self.group_cache
            .entry(gid)
            .or_insert_with(|| match get_group_by_gid(gid) {
                Some(res) => res.name().to_string_lossy().to_string(),
                None => gid.to_string(),
            })
    }
}
