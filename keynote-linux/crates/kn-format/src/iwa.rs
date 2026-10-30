//! One `Index/*.iwa` file: a stream of archives. Each archive is a TSP.ArchiveInfo header
//! (object identifier + one MessageInfo per payload) followed by the payload messages.

use crate::msg::{Msg, ObjId};
use crate::schema::schema;
use crate::wire::{iwa_frame, iwa_unframe, read_varint, write_varint};
use crate::Error;

#[derive(Debug, Clone)]
pub struct Archive {
    pub info: Msg,
    pub payloads: Vec<Msg>,
}

impl Archive {
    pub fn id(&self) -> ObjId {
        self.info.u64("identifier").unwrap_or(0)
    }

    /// Registry type id of payload `i`.
    pub fn type_id(&self, i: usize) -> u32 {
        self.info.msgs("message_infos").nth(i).and_then(|mi| mi.u32("type")).unwrap_or(0)
    }

    /// The object itself (first payload).
    pub fn obj(&self) -> &Msg {
        &self.payloads[0]
    }

    pub fn obj_mut(&mut self) -> &mut Msg {
        &mut self.payloads[0]
    }

    pub fn object_references(&self, i: usize) -> Vec<ObjId> {
        self.info.msgs("message_infos").nth(i).map(|mi| mi.u64s("object_references")).unwrap_or_default()
    }

    pub fn set_object_references(&mut self, i: usize, refs: &[ObjId]) {
        if let Some(mi) = self.info.msgs_mut("message_infos").nth(i) {
            mi.set_u64s_packed("object_references", refs);
        }
    }

    pub fn data_references(&self, i: usize) -> Vec<u64> {
        self.info.msgs("message_infos").nth(i).map(|mi| mi.u64s("data_references")).unwrap_or_default()
    }

    /// Declare an extension field of payload `i` (e.g. ImageArchive's gallery, path [200]) the
    /// way Keynote does: a Message field that older readers keep until they modify the object,
    /// with the objects and data it refers to. Replaces an earlier declaration of the path.
    pub fn set_field_info(&mut self, i: usize, path: &[u64], objects: &[u64], datas: &[u64]) {
        let Some(mi) = self.info.msgs_mut("message_infos").nth(i) else { return };
        mi.retain_msgs("field_infos", |f| f.msg("path").map(|p| p.u64s("path")) != Some(path.to_vec()));
        let fi = mi.push_new("field_infos");
        fi.msg_mut_or_create("path").set_u64s_packed("path", path);
        fi.set_enum("type", "Message");
        fi.set_enum("unknown_field_rule", "IgnoreAndPreserveUntilModified");
        if !objects.is_empty() {
            fi.set_u64s_packed("object_references", objects);
        }
        if !datas.is_empty() {
            fi.set_u64s_packed("data_references", datas);
        }
    }

    pub fn set_data_references(&mut self, i: usize, refs: &[u64]) {
        if let Some(mi) = self.info.msgs_mut("message_infos").nth(i) {
            mi.set_u64s_packed("data_references", refs);
        }
    }

    /// New archive holding a single object of the given type.
    pub fn new_object(id: ObjId, payload: Msg) -> Archive {
        let mut info = Msg::new("TSP.ArchiveInfo");
        info.set_u64("identifier", id);
        let mut mi = Msg::new("TSP.MessageInfo");
        let tid = schema().type_id_of(payload.type_name()).expect("payload type has registry id");
        mi.set_u64("type", u64::from(tid));
        mi.set_u64s_packed("version", &[1, 0, 5]);
        mi.set_u64("length", 0);
        info.push_msg("message_infos", mi);
        let mut a = Archive { info, payloads: vec![payload] };
        a.refresh_object_references();
        a
    }

    /// Recompute MessageInfo.object_references from the references inside the payloads.
    pub fn refresh_object_references(&mut self) {
        self.refresh_object_references_except(&[]);
    }

    /// Like [`Archive::refresh_object_references`], leaving out `skip` (Keynote doesn't list
    /// references to the stylesheet itself).
    pub fn refresh_object_references_except(&mut self, skip: &[ObjId]) {
        for i in 0..self.payloads.len() {
            let mut refs = Vec::new();
            self.payloads[i].collect_refs(&mut refs);
            refs.retain(|r| !skip.contains(r));
            refs.sort_unstable();
            refs.dedup();
            self.set_object_references(i, &refs);
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct IwaFile {
    pub archives: Vec<Archive>,
}

impl IwaFile {
    pub fn parse(raw: &[u8]) -> Result<IwaFile, Error> {
        let data = iwa_unframe(raw)?;
        let s = schema();
        let info_ty = s.type_idx("TSP.ArchiveInfo");
        let mut archives = Vec::new();
        let mut pos = 0;
        while pos < data.len() {
            let len = read_varint(&data, &mut pos)? as usize;
            let hdr = data.get(pos..pos + len).ok_or(Error::Corrupt("truncated ArchiveInfo"))?;
            let info = Msg::parse(hdr, info_ty)?;
            pos += len;
            let mut payloads = Vec::new();
            for mi in info.msgs("message_infos") {
                let mlen = mi.u64("length").unwrap_or(0) as usize;
                let ty = mi.u32("type").and_then(|t| s.for_type_id(t));
                let body = data.get(pos..pos + mlen).ok_or(Error::Corrupt("truncated payload"))?;
                let m = Msg::parse(body, ty).or_else(|_| Msg::parse(body, None))?;
                payloads.push(m);
                pos += mlen;
            }
            archives.push(Archive { info, payloads });
        }
        Ok(IwaFile { archives })
    }

    /// Decompressed stream (what `parse` reads after unframing).
    pub fn encode_stream(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for a in &self.archives {
            let mut info = a.info.clone();
            for (mi, p) in info.msgs_mut("message_infos").zip(&a.payloads) {
                let n = p.encoded_len() as u64;
                if mi.u64("length") != Some(n) {
                    mi.set_u64("length", n);
                }
            }
            let hdr = info.encode();
            write_varint(&mut out, hdr.len() as u64);
            out.extend(hdr);
            for p in &a.payloads {
                p.encode_into(&mut out);
            }
        }
        out
    }

    pub fn encode(&self) -> Vec<u8> {
        iwa_frame(&self.encode_stream())
    }
}
