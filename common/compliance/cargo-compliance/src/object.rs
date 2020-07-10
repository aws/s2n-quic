use crate::{annotation::AnnotationSet, parser::Parser, Error};
use goblin::{
    archive::Archive,
    elf::Elf,
    mach::{Mach, MachO, MultiArch},
    pe::PE,
    Object,
};

pub fn extract(buffer: &[u8], annotations: &mut AnnotationSet) -> Result<(), Error> {
    let object = Object::parse(buffer)?;
    (&object, buffer).load(annotations)?;

    Ok(())
}

trait AnnoObject {
    fn load(&self, annotations: &mut AnnotationSet) -> Result<(), Error>;
}

impl<'a> AnnoObject for (&Object<'a>, &'a [u8]) {
    fn load(&self, annotations: &mut AnnotationSet) -> Result<(), Error> {
        match &self.0 {
            Object::Elf(obj) => (obj, self.1).load(annotations),
            Object::PE(obj) => (obj, self.1).load(annotations),
            Object::Mach(obj) => (obj, self.1).load(annotations),
            Object::Archive(obj) => (obj, self.1).load(annotations),
            _ => Err("Unknown file format".to_string().into()),
        }
    }
}

impl<'a> AnnoObject for (&Elf<'a>, &'a [u8]) {
    fn load(&self, annotations: &mut AnnotationSet) -> Result<(), Error> {
        let elf = &self.0;
        for sect in &elf.section_headers {
            if sect.sh_type != goblin::elf::section_header::SHT_NOTE {
                continue;
            }

            if !elf
                .shdr_strtab
                .get(sect.sh_name)
                .map_or(false, |r| r.ok() == Some(".note.compliance"))
            {
                continue;
            }

            let addr = sect.sh_offset as usize;
            let len = sect.sh_size as usize;
            for annotation in Parser(&self.1[addr..(addr + len)]) {
                annotations.insert(annotation?);
            }
        }

        Ok(())
    }
}

impl<'a> AnnoObject for (&PE<'a>, &'a [u8]) {
    fn load(&self, annotations: &mut AnnotationSet) -> Result<(), Error> {
        for section in &self.0.sections {
            if Some(".debug_compliance") == section.real_name.as_deref() {
                let addr = section.pointer_to_raw_data as usize;
                let len = section.virtual_size as usize;

                for annotation in Parser(&self.1[addr..(addr + len)]) {
                    annotations.insert(annotation?);
                }
            }
        }
        Ok(())
    }
}

impl<'a> AnnoObject for (&Mach<'a>, &'a [u8]) {
    fn load(&self, annotations: &mut AnnotationSet) -> Result<(), Error> {
        match &self.0 {
            Mach::Fat(obj) => (obj, self.1).load(annotations),
            Mach::Binary(obj) => obj.load(annotations),
        }
    }
}

impl<'a> AnnoObject for (&MultiArch<'a>, &'a [u8]) {
    fn load(&self, annotations: &mut AnnotationSet) -> Result<(), Error> {
        for arch in self.0.iter_arches() {
            let arch = arch?;
            extract(arch.slice(self.1), annotations)?;
        }
        Ok(())
    }
}

impl<'a> AnnoObject for MachO<'a> {
    fn load(&self, annotations: &mut AnnotationSet) -> Result<(), Error> {
        for sections in self.segments.sections() {
            for section in sections {
                if let Ok((section, data)) = section {
                    if let (b"__DATA\0\0\0\0\0\0\0\0\0\0", b"__compliance\0\0\0\0") =
                        (&section.segname, &section.sectname)
                    {
                        for annotation in Parser(data) {
                            annotations.insert(annotation?);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

impl<'a> AnnoObject for (&Archive<'a>, &'a [u8]) {
    fn load(&self, annotations: &mut AnnotationSet) -> Result<(), Error> {
        for member in self.0.members() {
            if let Ok(contents) = self.0.extract(member, self.1) {
                let _ = extract(contents, annotations);
            }
        }
        Ok(())
    }
}
