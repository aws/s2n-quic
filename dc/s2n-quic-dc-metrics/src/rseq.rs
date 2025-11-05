use std::{
    cell::Cell,
    ffi::CStr,
    fs,
    mem::MaybeUninit,
    ptr::NonNull,
    sync::{
        atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering},
        Mutex,
    },
};

const SLOTS: usize = 1024 * 8 - 1;

#[repr(C, align(128))]
struct Page {
    // assembly assumes slots is at index 0
    slots: [MaybeUninit<u64>; SLOTS],
    length: AtomicU64,
}

impl Page {
    fn new() -> Box<Page> {
        Box::new(Page {
            slots: [const { MaybeUninit::uninit() }; SLOTS],
            length: AtomicU64::new(0),
        })
    }
}

fn possible_cpus() -> usize {
    let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/possible") else {
        // As a fallback, ask Rust to provide us how much parallelism we have. This is **not**
        // the best option because there's no guarantee the parallelism matches up with the CPU
        // indices returned by the kernel, but in practice in our environments it's fairly likely
        // to do what we want.
        if let Ok(parallelism) = std::thread::available_parallelism() {
            return parallelism.get();
        } else {
            static PRINTED_WARNING: AtomicBool = AtomicBool::new(false);
            if !PRINTED_WARNING.swap(true, Ordering::Relaxed) {
                eprintln!("failed to identify CPU count, falling back to 4 fast CPUs");
            }
            // If neither option worked, default to 4 CPU cores. This essentially means that we
            // will have great performance on those 4 cores and terrible performance elsewhere.
            //
            // Our critical section will bail out to the fallback path if we're on a CPU with index
            // larger than this, so this really just an arbitrary value.
            return 4;
        }
    };

    let max_cpu = content
        .trim()
        .split(',')
        .map(|range| {
            if let Some((_start, end)) = range.split_once('-') {
                end.parse::<usize>().unwrap_or(0)
            } else {
                range.parse::<usize>().unwrap_or(0)
            }
        })
        .max()
        .unwrap_or(0);

    max_cpu.max(1)
}

fn init_per_cpu() -> Box<[AtomicPtr<Page>]> {
    (0..=possible_cpus())
        .map(|_| AtomicPtr::new(std::ptr::null_mut()))
        .collect()
}

/// Each CPU core populates a `page` until it fills up, and then pushes events into aggregate.
///
/// We also support stealing pages from all CPUs (`steal_pages`). Without that mechanism we'd leave
/// behind metrics on a CPU core that did some work and then went idle.
///
/// `T` is a type which aggregates incoming events. `T`'s are allocated typically per registered
/// metric, and when we're absorbing a page of events we'll do so under the `aggregate` lock.
///
/// This does mean that if events are flowing at a very high rate there may be contention on
/// `aggregate`; we retain the lock for now for simplicity. We could have a background thread that
/// aggregates events, but then we'd either need to drop events or have unbounded memory. We could
/// also have a shared-atomic aggregate, but that increases memory usage or requires another
/// somewhat complicated data structure (per-index Arc / Vec with lock-free access respectively).
pub(crate) struct Channels<T: Absorb> {
    per_cpu: Box<[AtomicPtr<Page>]>,

    /// If true, it's not possible for us to use the per_cpu events. This primarily happens if we
    /// fail to register to use membarrier. Note that even if false fallback may still be used
    /// (e.g. because we fail to register rseq).
    must_use_fallback: bool,

    fallback: parking_lot::RwLock<crossbeam_queue::SegQueue<u64>>,

    empty_pages: crossbeam_queue::SegQueue<Box<Page>>,

    // What we aggregate events into.
    aggregate: Mutex<Vec<T>>,
}

impl<T: Absorb> Drop for Channels<T> {
    fn drop(&mut self) {
        // Make sure we don't leak pages.
        self.steal_pages();
    }
}

pub(crate) trait Absorb: Sized + Default {
    fn handle(slots: &mut [Self], events: &mut [u64]);
}

static PRINTED_MEMBARRIER_WARNING: AtomicBool = AtomicBool::new(false);

impl<T: Absorb> Channels<T> {
    #[cfg_attr(not(target_os = "linux"), allow(unused_assignments))]
    pub(crate) fn new() -> Self {
        let mut must_use_fallback = false;

        #[cfg(target_os = "linux")]
        {
            let ret = unsafe {
                libc::syscall(
                    libc::SYS_membarrier,
                    libc::MEMBARRIER_CMD_REGISTER_PRIVATE_EXPEDITED,
                    0,
                )
            };
            if ret != 0 {
                if !PRINTED_MEMBARRIER_WARNING.swap(true, Ordering::Relaxed) {
                    eprintln!(
                        "failed to register membarrier: {:?}, {:?}",
                        ret,
                        std::io::Error::last_os_error()
                    );
                }
                must_use_fallback = true;
            }

            #[cfg(target_arch = "aarch64")]
            if !std::arch::is_aarch64_feature_detected!("lse") {
                must_use_fallback = true;
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            must_use_fallback = true;
        }

        Channels {
            must_use_fallback,
            per_cpu: init_per_cpu(),
            fallback: Default::default(),
            empty_pages: Default::default(),

            aggregate: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn get_mut<R>(&self, idx: u32, mut cb: impl FnMut(&mut T) -> R) -> R {
        let mut guard = self.aggregate.lock().unwrap();
        cb(&mut guard[idx as usize])
    }

    pub(crate) fn allocate(&self) -> u32 {
        let mut guard = self.aggregate.lock().unwrap();
        let len = u32::try_from(guard.len()).unwrap();
        guard.push(T::default());
        len
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn steal_pages(&self) {
        self.aggregate_fallback(true);
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn steal_pages(&self) {
        if self.must_use_fallback {
            self.aggregate_fallback(true);

            // Don't look at per_cpu structures if we're in the only-fallback path.
            return;
        }

        let pages = self
            .per_cpu
            .iter()
            .map(|cpu| cpu.swap(std::ptr::null_mut(), Ordering::Relaxed))
            .collect::<Vec<_>>();

        // In theory this is infallible because we successfully registered membarrier above.
        //
        // If this fails it's UB to read from the stolen pages since we don't actually own them.
        // For now treat failure as a fatal condition and abort the process since there's no good
        // recovery path. If we're willing to leak a few sets of pages, it's probably possible to
        // leak these pages and then instruct all CPUs to use a stronger memory ordering (primarily
        // on aarch64) when finishing writes to the pages. But that doesn't seem obviously better
        // given the assumption this is infallible.
        //
        // FIXME: We should confirm via benchmarks that we actually need this. On x86_64 if we
        // preallocate all Pages the `mov` to increment length is implicitly a Release we could
        // Acquire synchronize with here. On aarch64 though we'd need to use a stronger memory
        // ordering instruction which is more expensive (maybe unaffordably so).
        let ret = unsafe {
            libc::syscall(
                libc::SYS_membarrier,
                libc::MEMBARRIER_CMD_PRIVATE_EXPEDITED,
                0,
            )
        };
        if ret != 0 {
            eprintln!(
                "failed to membarrier: {:?}, {:?}",
                ret,
                std::io::Error::last_os_error()
            );
            std::process::abort();
        }

        // All other CPUs have now flushed their memory stores and are guaranteed to
        // exit any ongoing RSEQ sections too due to membarrier. This means that (a) our thread
        // will see any events they've written and (b) they are no longer writing to the pages in
        // `PER_CPU`, which is sufficient to allow us to process all the events they've sent.

        for page in pages {
            if !page.is_null() {
                self.handle_events(unsafe { Box::from_raw(page) });
            }
        }

        self.aggregate_fallback(true);
    }

    fn handle_events(&self, mut page: Box<Page>) {
        let mut aggregate = self.aggregate.lock().unwrap();
        let length = *page.length.get_mut() as usize;
        let filled = unsafe { &mut *(&mut page.slots[..length] as *mut [_] as *mut [u64]) };
        T::handle(&mut aggregate, filled);
        drop(aggregate);

        *page.length.get_mut() = 0;
        self.empty_pages.push(page);
    }

    #[cfg(not(target_os = "linux"))]
    pub(crate) fn send_event(&self, event: u64) {
        self.fallback_push(event)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn send_event(&self, event: u64) {
        if self.must_use_fallback {
            return self.fallback_push(event);
        }

        let rseq_ptr = rseq();
        self.send_event_inner(event, rseq_ptr);
    }

    // Separate function for unit testing.
    #[cfg(target_os = "linux")]
    fn send_event_inner(&self, event: u64, rseq_ptr: NonNull<Rseq>) {
        unsafe {
            #[cfg(target_arch = "x86_64")]
            std::arch::asm!(
                "
                .pushsection __rseq_cs, \"aw\"
                .balign 32
                9:
                .long 0
                .long 0
                .quad 2f
                .quad (6f-2f)
                .quad 7f
                .popsection

                // The kernel ABI requires that the address we abort to is prefixed with the
                // RSEQ_SIG. This reduces the likelihood that writes to the descriptor block
                // (declared above) allow branching to arbitrary addresses in the code which makes
                // things easier for attackers.
                //
                // In the future we can optimize by moving the abort sequence out of the
                // primary instruction stream somehow so that we don't need this unconditional
                // jump.
                jmp 7f
                .long {RSEQ_SIG}
                7:
                mov {cpu_id:e}, [{rseq_ptr}+{cpu_id_offset_start}]

                // If the CPU index returned by rseq is too high, then we bail out
                // to our fallback path. This also handles the case that rseq failed (-1 or
                // u32::MAX is definitely out of range).
                cmp {cpu_id}, {per_cpu_len}
                jge {fallback}

                // Only attempt looping through rseq a limited number of times to make progress if
                // we're continuously aborting for some reason.
                dec {loop_count}
                jz {fallback}

                lea {tmp}, [rip+9b]
                mov [{rseq_ptr}+{rseq_cs_offset}], {tmp}

                // Everything following this is the critical section. It must be capable of
                // restarting after any instruction except the last one with no harmful effects.
                2:

                // Check that the CPU ID matches with the one loaded above.
                //
                // See https://google.github.io/tcmalloc/rseq.html#cpu-ids for good docs on why
                // there's two cpu_id fields.
                cmp {cpu_id:e}, [{rseq_ptr}+{cpu_id_offset}]
                jnz 7b

                mov {page_ptr}, [{per_cpu_base}+{cpu_id}*8]
                test {page_ptr}, {page_ptr}
                jz {needs_new_page}

                mov {tmp}, [{page_ptr}+{length_offset}]
                cmp {tmp}, {SLOTS}
                jge {needs_new_page}

                // page is non-null + non-empty

                // write the event to the current slot
                mov [{page_ptr}+{tmp}*8], {event}

                // increment the length
                inc {tmp}

                // length update must be the last instruction,
                // and must be a relaxed atomic store.
                mov [{page_ptr}+{length_offset}], {tmp}
                6:

                // Clear the rseq block as being in the critical section.
                // AFAICT, this isn't required, but tcmalloc's docs recommend it and it's
                // relatively cheap.
                mov QWORD PTR [{rseq_ptr}+{rseq_cs_offset}], 0
                ",
                rseq_ptr = in(reg) rseq_ptr.as_ptr(),
                cpu_id = out(reg) _,
                page_ptr = out(reg) _,
                tmp = out(reg) _,
                loop_count = inout(reg) 5u64 => _,
                per_cpu_base = in(reg) self.per_cpu.as_ptr(),
                per_cpu_len = in(reg) self.per_cpu.len(),
                event = in(reg) event,
                cpu_id_offset = const std::mem::offset_of!(Rseq, cpu_id),
                cpu_id_offset_start = const std::mem::offset_of!(Rseq, cpu_id_start),
                rseq_cs_offset = const std::mem::offset_of!(Rseq, rseq_cs),
                length_offset = const std::mem::offset_of!(Page, length),
                RSEQ_SIG = const RSEQ_SIG,
                SLOTS = const SLOTS,
                needs_new_page = label {
                    self.send_event_slow(rseq_ptr, event);
                },
                fallback = label {
                    self.fallback_push(event);
                },
                options(nostack)
            );

            #[cfg(target_arch = "aarch64")]
            std::arch::asm!(
                "
                .pushsection __rseq_cs, \"aw\"
                .balign 32
                9:
                .long 0
                .long 0
                .quad 2f
                .quad (6f-2f)
                .quad 7f
                .popsection

                b 7f
                .long {RSEQ_SIG}
                7:
                ldr {cpu_id:w}, [{rseq_ptr}, #{cpu_id_offset_start}]

                cmp {cpu_id:w}, {per_cpu_len:w}
                b.ge {fallback}

                subs {loop_count}, {loop_count}, #1
                b.eq {fallback}

                adrp {tmp}, 9b
                ldr {tmp}, [{tmp}, #:lo12:9b]
                str {tmp}, [{rseq_ptr}, #{rseq_cs_offset}]

                2:
                ldr {tmp:w}, [{rseq_ptr}, #{cpu_id_offset}]
                cmp {cpu_id:w}, {tmp:w}
                b.ne 7b

                ldr {page_ptr}, [{per_cpu_base}, {cpu_id}, lsl #3]
                cbz {page_ptr}, {needs_new_page}

                ldr {tmp}, [{page_ptr}, {length_offset}]
                cmp {tmp}, {SLOTS}
                b.ge {needs_new_page}

                str {event}, [{page_ptr}, {tmp}, lsl #3]

                add {tmp}, {tmp}, #1

                str {tmp}, [{page_ptr}, {length_offset}]
                6:
                str xzr, [{rseq_ptr}, #{rseq_cs_offset}]
                ",
                rseq_ptr = in(reg) rseq_ptr.as_ptr(),
                cpu_id = out(reg) _,
                page_ptr = out(reg) _,
                tmp = out(reg) _,
                loop_count = inout(reg) 5u64 => _,
                per_cpu_base = in(reg) self.per_cpu.as_ptr(),
                per_cpu_len = in(reg) self.per_cpu.len(),
                event = in(reg) event,
                cpu_id_offset = const std::mem::offset_of!(Rseq, cpu_id),
                cpu_id_offset_start = const std::mem::offset_of!(Rseq, cpu_id_start),
                rseq_cs_offset = const std::mem::offset_of!(Rseq, rseq_cs),
                // too large for constant offset
                length_offset = in(reg) std::mem::offset_of!(Page, length),
                RSEQ_SIG = const RSEQ_SIG,
                // too large for constant
                SLOTS = in(reg) SLOTS,
                needs_new_page = label {
                    // SAFETY: lse is feature detected above and fallback is forced if it's not
                    // present, which means we never hit this code.
                    //
                    // lse is present on all aarch64 CPUs we'd expect run on (Graviton 2 has it).
                    #[allow(unused_unsafe)]
                    unsafe {
                        self.send_event_slow(rseq_ptr, event);
                    }
                },
                fallback = label {
                    self.fallback_push(event);
                },
                options(nostack)
            );
        }
    }

    #[inline(never)]
    fn fallback_push(&self, event: u64) {
        let read_guard = self.fallback.read();
        read_guard.push(event);
        if read_guard.len() < SLOTS * 2 {
            return;
        }
        drop(read_guard);

        self.aggregate_fallback(false);
    }

    fn aggregate_fallback(&self, force: bool) {
        let mut write_guard = self.fallback.write();
        if !force && write_guard.len() < SLOTS * 2 {
            return;
        }
        let taken = std::mem::take(&mut *write_guard);
        drop(write_guard);

        let mut buffer = Vec::with_capacity(SLOTS * 2);
        taken.into_iter().for_each(|e| buffer.push(e));
        let mut aggregate = self.aggregate.lock().unwrap();
        T::handle(&mut aggregate, &mut buffer);
        drop(aggregate);
    }

    #[cold]
    #[cfg(target_os = "linux")]
    #[cfg_attr(target_arch = "aarch64", target_feature(enable = "lse"))]
    fn send_event_slow(&self, rseq_ptr: NonNull<Rseq>, serialized_event: u64) {
        let mut new_page = self.empty_pages.pop().unwrap_or_else(Page::new);

        new_page.slots[0].write(serialized_event);
        new_page.length.store(1, Ordering::Relaxed);

        let mut taken: *mut Page = Box::into_raw(new_page);

        let mut fallback: u8 = 0;
        unsafe {
            #[cfg(target_arch = "x86_64")]
            std::arch::asm!(
                "
                .pushsection __rseq_cs, \"aw\"
                .balign 32
                12:
                .long 0
                .long 0
                .quad 3f
                .quad (7f-3f)
                .quad 8f
                .popsection

                jmp 8f
                .long {RSEQ_SIG}
                8:
                mov {cpu_id:e}, [{rseq_ptr}+{cpu_id_offset_start}]

                // If the CPU index returned by rseq is too high, then we bail out
                // to our fallback path. This also handles the case that rseq failed (-1 or
                // u32::MAX is definitely out of range).
                cmp {cpu_id}, {per_cpu_len}
                setge {fallback}
                jge 7f

                // Only attempt looping through rseq a limited number of times to make progress if
                // we're continuously aborting for some reason.
                dec {loop_count}
                setz {fallback}
                jz 7f

                lea {tmp}, [rip+12b]
                mov [{rseq_ptr}+{rseq_cs_offset}], {tmp}

                3:
                cmp {cpu_id:e}, [{rseq_ptr}+{cpu_id_offset}]
                jnz 8b

                xchg {taken}, [{per_cpu_base}+{cpu_id}*8]

                7:
                mov QWORD PTR [{rseq_ptr}+{rseq_cs_offset}], 0
                ",
                rseq_ptr = in(reg) rseq_ptr.as_ptr(),
                cpu_id = out(reg) _,
                tmp = out(reg) _,
                loop_count = inout(reg) 5u64 => _,
                per_cpu_base = in(reg) self.per_cpu.as_ptr(),
                per_cpu_len = in(reg) self.per_cpu.len(),
                taken = inout(reg) taken,
                cpu_id_offset = const std::mem::offset_of!(Rseq, cpu_id),
                cpu_id_offset_start = const std::mem::offset_of!(Rseq, cpu_id_start),
                rseq_cs_offset = const std::mem::offset_of!(Rseq, rseq_cs),
                RSEQ_SIG = const RSEQ_SIG,
                fallback = inout(reg_byte) fallback,
                options(nostack)
            );

            #[cfg(target_arch = "aarch64")]
            std::arch::asm!(
                "
                .pushsection __rseq_cs, \"aw\"
                .balign 32
                12:
                .long 0
                .long 0
                .quad 3f
                .quad (7f-3f)
                .quad 8f
                .popsection

                b 8f
                .long {RSEQ_SIG}
                8:
                ldr {cpu_id:w}, [{rseq_ptr}, #{cpu_id_offset_start}]

                cmp {cpu_id:w}, {per_cpu_len:w}
                cset {fallback:w}, ge
                b.ge 7f

                subs {loop_count}, {loop_count}, #1
                cset {fallback:w}, eq
                b.eq 7f

                adrp {tmp}, 12b
                ldr {tmp}, [{tmp}, #:lo12:12b]
                str {tmp}, [{rseq_ptr}, #{rseq_cs_offset}]

                3:
                ldr {tmp2:w}, [{rseq_ptr}, #{cpu_id_offset}]
                cmp {cpu_id:w}, {tmp2:w}
                b.ne 8b

                add {tmp}, {per_cpu_base}, {cpu_id}, lsl #3
                swp {taken}, {taken}, [{tmp}]

                7:
                str xzr, [{rseq_ptr}, #{rseq_cs_offset}]
                ",
                rseq_ptr = in(reg) rseq_ptr.as_ptr(),
                cpu_id = out(reg) _,
                tmp = out(reg) _,
                tmp2 = out(reg) _,
                loop_count = inout(reg) 5u64 => _,
                per_cpu_base = in(reg) self.per_cpu.as_ptr(),
                per_cpu_len = in(reg) self.per_cpu.len(),
                taken = inout(reg) taken,
                cpu_id_offset = const std::mem::offset_of!(Rseq, cpu_id),
                cpu_id_offset_start = const std::mem::offset_of!(Rseq, cpu_id_start),
                rseq_cs_offset = const std::mem::offset_of!(Rseq, rseq_cs),
                RSEQ_SIG = const RSEQ_SIG,
                fallback = inout(reg) fallback,
                options(nostack)
            );
        }
        let fallback = fallback != 0;

        if fallback {
            // Because we hit fallback there shouldn't have been opportunity for the code to
            // exchange, so it shouldn't be possible for it to be null.
            assert!(!taken.is_null());

            // We failed to xchg `taken` with the page in the per_cpu[current] slot. As such
            // `taken` is still owned by us:
            let taken = unsafe { Box::from_raw(taken) };

            // And we need to handle events from it. This is a *very* slow path, but should
            // basically never happen in production; if we're consistently hitting the fallback
            // path we should be hitting it in the hot path too, which will then bail to the faster
            // fallback (`fallback_push`) rather than here.

            self.handle_events(taken);
        } else {
            if taken.is_null() {
                return;
            }

            self.handle_events(unsafe { Box::from_raw(taken) });
        }
    }
}

thread_local! {
    static RSEQ: Cell<Option<NonNull<Rseq>>> = const { Cell::new(None) };

    static RSEQ_ALLOC: Cell<Option<RseqStorage>> = const { Cell::new(None) };
}

struct RseqStorage {
    slot: Box<Rseq>,
    registered: bool,
}

impl Drop for RseqStorage {
    fn drop(&mut self) {
        let Some(taken_address) = RSEQ.take() else {
            return;
        };

        if !self.registered {
            return;
        }

        // Unregister rseq before we free the memory. This avoids the kernel writing to it while
        // the thread is dying and clobbering something else that happens to get allocated there.
        if let Err(e) = sys_rseq(
            taken_address.as_ptr(),
            1i32, /* RSEQ_FLAGS_UNREGISTER */
        ) {
            eprintln!("failed to deregister rseq on thread death: {e:?}");
        }
    }
}

#[repr(C)]
#[repr(align(32))]
pub(crate) struct Rseq {
    cpu_id_start: u32,
    cpu_id: u32,
    rseq_cs: u64,
    flags: u32,
}

// Note that NonNull is !Send + !Sync, so the compiler protects us against accessing this
// cross-thread.
pub(crate) fn rseq() -> NonNull<Rseq> {
    if let Some(ptr) = RSEQ.get() {
        return ptr;
    }

    rseq_init()
}

// Note that this is defined by glibc for both x86_64 and aarch64 as it owns rseq registration on
// AL2023+ (see /usr/include/bits/rseq.h on an AL2023 system).
//
// For AL2 for simplicity we use the same RSEQ constant.
//
// This is part of the glibc ABI, we can't influence this value in any way.
#[cfg(target_arch = "x86_64")]
const RSEQ_SIG: u32 = 0x53053053;

#[cfg(target_arch = "aarch64")]
const RSEQ_SIG: u32 = 0xd428bc00;

#[cfg(test)]
static RSEQ_FAILED: AtomicBool = AtomicBool::new(false);

#[cold]
fn rseq_init() -> NonNull<Rseq> {
    // If we successfully fetch rseq from libc, we **don't** touch the RSEQ_ALLOC thread local at
    // all, which means there's nothing to deregister or drop.
    //
    // Note that caching the value of rseq like this is possibly error prone if there's access to
    // RSEQ during thread death, when the thread locals are dropped in ~random order. But there's
    // not too much we can do about that, glibc doesn't provide an interface that lets us check
    // whether the thread local it allocates is still around. So just assume that's not an issue.
    // A working theory is that glibc doesn't reuse the thread local memory while the thread is
    // still alive.
    if let Ok(libc_rseq) = from_libc() {
        let ptr = NonNull::new(libc_rseq).unwrap();
        RSEQ.set(Some(ptr));
        return ptr;
    }

    // Register the main thread with rseq.
    let mut rseq_ptr = RseqStorage {
        slot: Box::new(Rseq {
            cpu_id_start: 0,
            cpu_id: 0,
            rseq_cs: 0,
            flags: 0,
        }),
        registered: false,
    };

    RSEQ.set(Some(NonNull::new(&raw mut *rseq_ptr.slot).unwrap()));
    RSEQ_ALLOC.set(Some(rseq_ptr));
    let rseq_ptr = RSEQ.get().unwrap();

    match sys_rseq(rseq_ptr.as_ptr(), 0) {
        Ok(()) => {
            RSEQ_ALLOC.with(|c| {
                let mut v = c.take().expect("just set above");
                v.registered = true;
                c.set(Some(v));
            });
        }
        Err(e) => {
            eprintln!("rseq failed to register: {e:?}");
            // Mark the structure as unregistered.
            //
            // In theory the kernel has done this but this helps make that a stronger
            // guarantee. This is necessary so that our assembly will bail out to the fallback
            // path rather than e.g. all threads thinking they are on CPU 0.
            #[cfg(test)]
            RSEQ_FAILED.store(true, Ordering::Relaxed);
            unsafe {
                (*rseq_ptr.as_ptr()).cpu_id_start = u32::MAX;
            }
        }
    };

    rseq_ptr
}

fn dlsym(symbol: &CStr) -> std::io::Result<*mut std::ffi::c_void> {
    unsafe {
        // clear previous errors
        let _ = libc::dlerror();
        let address = libc::dlsym(libc::RTLD_DEFAULT, symbol.as_ptr());
        if let Some(ptr) = NonNull::new(libc::dlerror()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "failed to dlsym {symbol:?}: {:?}",
                    std::ffi::CStr::from_ptr(ptr.as_ptr())
                ),
            ));
        }
        Ok(address)
    }
}

fn thread_plus_offset(offset: libc::ptrdiff_t) -> *mut std::ffi::c_void {
    let output: *mut std::ffi::c_void;
    // As far as I can tell, both of these should work in the most general case.
    //
    // Online references suggest you need e.g. __tls_get_addr and similar, but it seems to me that
    // glibc's __rseq_offset already takes care of calling that for us if needed, so we just need
    // the base pointer which is already conveniently stored in a register for us.
    unsafe {
        #[cfg(target_arch = "aarch64")]
        std::arch::asm!("mrs {output}, tpidr_el0", output = out(reg) output);

        #[cfg(target_arch = "x86_64")]
        std::arch::asm!("mov {output}, fs:0", output = out(reg) output);
    }
    output.wrapping_offset(offset)
}

fn from_libc() -> std::io::Result<*mut Rseq> {
    let _size = dlsym(c"__rseq_size")?.cast::<u32>();
    let offset = dlsym(c"__rseq_offset")?.cast::<libc::ptrdiff_t>(); // ptrdiff_t
    let _flags = dlsym(c"__rseq_flags")?.cast::<u32>();

    Ok(thread_plus_offset(unsafe { offset.read() }).cast())
}

#[allow(clippy::needless_return)]
fn sys_rseq(rseq_abi: *mut Rseq, flags: i32) -> std::io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let ret = unsafe {
            libc::syscall(
                libc::SYS_rseq,
                rseq_abi,
                std::mem::size_of::<Rseq>() as u32,
                flags,
                RSEQ_SIG,
            )
        };
        if ret != 0 {
            return Err(std::io::Error::last_os_error());
        }

        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // See comments in possible_cpus if this fails -- it's possible the failure just indicates we
    // need to ignore that particular test environment.
    #[test]
    fn check_per_cpu() {
        let per_cpu = init_per_cpu();
        assert!(per_cpu.len() >= std::thread::available_parallelism().unwrap().get());
    }

    #[derive(Default)]
    struct TestAbsorber {
        value: u64,
    }

    impl super::Absorb for TestAbsorber {
        fn handle(slots: &mut [Self], events: &mut [u64]) {
            slots[0].value += events.len() as u64;
        }
    }

    #[test]
    fn test_send_event_local() {
        let mut channels = Channels::<TestAbsorber>::new();

        channels.allocate();

        for idx in 0..30 {
            channels.send_event(idx);
        }

        // We don't expect this to pass if we're not using rseq. We check this after sending the
        // events since that happens later.
        if channels.must_use_fallback || RSEQ_FAILED.load(Ordering::Relaxed) {
            return;
        }

        let local_filled = channels
            .per_cpu
            .iter_mut()
            .filter_map(|cpu| std::ptr::NonNull::new(*cpu.get_mut()))
            .map(|cpu| unsafe { &mut *cpu.as_ptr() })
            .map(|cpu| *cpu.length.get_mut())
            .sum::<u64>();
        assert_eq!(local_filled, 30);

        channels.steal_pages();

        // After stealing all per-CPU pages are empty and the aggregate value is populated.
        assert!(channels.per_cpu.iter_mut().all(|c| c.get_mut().is_null()));
        assert_eq!(channels.get_mut(0, |x| x.value), 30);
    }

    #[test]
    fn test_send_event_overflow() {
        let channels = Channels::<TestAbsorber>::new();

        channels.allocate();

        // Guarantee enough writes that at least one page needs to overflow.
        let total_events = channels.per_cpu.len() * SLOTS;
        for _ in 0..total_events {
            channels.send_event(0);
        }

        channels.steal_pages();

        let count = channels.get_mut(0, |v| v.value);
        assert_eq!(count, total_events as u64);
    }

    #[test]
    fn test_thread_ctor_dtor() {
        // Confirms we are able to register + unregister for new threads
        std::thread::spawn(move || {
            rseq();
        })
        .join()
        .unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn check_send_branches() {
        let mut rseq = Rseq {
            // cpu index changed should force us to branch out to fallback.
            cpu_id_start: 0u32,
            cpu_id: 0u32,
            rseq_cs: 0u64,
            flags: 0u32,
        };

        // With a mismatched CPU index, we branch out to fallback.
        //
        // Note that this happens after N loops through the code, but we don't have any good way to
        // determine that unfortunately. But this does ensure we don't hang if the kernel isn't
        // cooperating.
        let channels = Channels::<TestAbsorber>::new();
        channels.allocate();
        rseq.cpu_id_start = 0;
        rseq.cpu_id = 1;
        assert_eq!(channels.fallback.read().len(), 0);
        channels.send_event_inner(0u64, NonNull::from(&mut rseq));
        assert_eq!(channels.fallback.read().len(), 1);
        drop(channels);

        // An index that's one out of bounds (but consistent) also causes us to fallback.
        let channels = Channels::<TestAbsorber>::new();
        channels.allocate();
        rseq.cpu_id_start = channels.per_cpu.len() as u32;
        rseq.cpu_id = channels.per_cpu.len() as u32;
        assert_eq!(channels.fallback.read().len(), 0);
        channels.send_event_inner(0u64, NonNull::from(&mut rseq));
        assert_eq!(channels.fallback.read().len(), 1);
        drop(channels);

        // We allocate the page at the right CPU index.
        let mut channels = Channels::<TestAbsorber>::new();
        channels.allocate();
        // Force 64 CPUs regardless of what hardware we're running on.
        channels.per_cpu = (0..=64)
            .map(|_| AtomicPtr::new(std::ptr::null_mut()))
            .collect();
        for idx in 0..64 {
            rseq.cpu_id_start = idx;
            rseq.cpu_id = idx;
            assert_eq!(channels.fallback.read().len(), 0);
            channels.send_event_inner(0u64, NonNull::from(&mut rseq));

            // Didn't fallback.
            assert_eq!(channels.fallback.read().len(), 0);

            // Only allocated exactly one CPU, at the right index.
            let taken = std::mem::take(channels.per_cpu[idx as usize].get_mut());
            assert!(!taken.is_null());
            drop(unsafe { Box::from_raw(taken) });
            for cpu in channels.per_cpu.iter_mut() {
                assert!(cpu.get_mut().is_null());
            }
        }
        drop(channels);
    }

    #[cfg(target_os = "linux")]
    #[test]
    // `lse` enablement on aarch64
    #[allow(unused_unsafe)]
    fn check_send_slow_branches() {
        #[cfg(target_arch = "aarch64")]
        if !std::arch::is_aarch64_feature_detected!("lse") {
            return;
        }

        let mut rseq = Rseq {
            // cpu index changed should force us to branch out to fallback.
            cpu_id_start: 0u32,
            cpu_id: 0u32,
            rseq_cs: 0u64,
            flags: 0u32,
        };

        // With a mismatched CPU index, we branch out to fallback.
        //
        // Note that this happens after N loops through the code, but we don't have any good way to
        // determine that unfortunately. But this does ensure we don't hang if the kernel isn't
        // cooperating.
        let channels = Channels::<TestAbsorber>::new();
        channels.allocate();
        rseq.cpu_id_start = 0;
        rseq.cpu_id = 1;
        unsafe {
            channels.send_event_slow(NonNull::from(&mut rseq), 0u64);
        }
        assert_eq!(channels.get_mut(0, std::mem::take).value, 1);
        drop(channels);

        // An index that's one out of bounds (but consistent) also causes us to fallback.
        let channels = Channels::<TestAbsorber>::new();
        channels.allocate();
        rseq.cpu_id_start = channels.per_cpu.len() as u32;
        rseq.cpu_id = channels.per_cpu.len() as u32;
        unsafe {
            channels.send_event_slow(NonNull::from(&mut rseq), 0u64);
        }
        assert_eq!(channels.get_mut(0, std::mem::take).value, 1);
        drop(channels);

        // We allocate the page at the right CPU index.
        let mut channels = Channels::<TestAbsorber>::new();
        channels.allocate();
        // Force 64 CPUs regardless of what hardware we're running on.
        channels.per_cpu = (0..=64)
            .map(|_| AtomicPtr::new(std::ptr::null_mut()))
            .collect();
        for idx in 0..64 {
            rseq.cpu_id_start = idx;
            rseq.cpu_id = idx;
            unsafe {
                channels.send_event_slow(NonNull::from(&mut rseq), 0u64);
            }

            // Didn't fallback.
            assert_eq!(channels.fallback.read().len(), 0);

            // And no events were sent on the channel -- we allocated a page and added it (this is
            // actually sort of tested above) but because no page existed, there wasn't anything to
            // take out and send.
            assert_eq!(channels.get_mut(0, std::mem::take).value, 0);

            // Only allocated exactly one CPU, at the right index.
            assert!(!channels.per_cpu[idx as usize].get_mut().is_null());
            for (cpu_idx, cpu) in channels.per_cpu.iter_mut().enumerate() {
                if cpu_idx == idx as usize {
                    continue;
                }
                assert!(cpu.get_mut().is_null());
            }

            // Repeating the slow-send on the same CPU *will* persist exactly one event.
            unsafe {
                channels.send_event_slow(NonNull::from(&mut rseq), 0u64);
            }

            // Didn't fallback.
            assert_eq!(channels.fallback.read().len(), 0);

            // And no events were sent on the channel -- we allocated a page and added it (this is
            // actually sort of tested above) but because no page existed, there wasn't anything to
            // take out and send.
            assert_eq!(channels.get_mut(0, std::mem::take).value, 1);

            // And a new page is now persisted.
            let taken = std::mem::take(channels.per_cpu[idx as usize].get_mut());
            assert!(!taken.is_null());
            drop(unsafe { Box::from_raw(taken) });
            for cpu in channels.per_cpu.iter_mut() {
                assert!(cpu.get_mut().is_null());
            }
            assert_eq!(channels.empty_pages.len(), 1);
        }
        drop(channels);
    }
}
