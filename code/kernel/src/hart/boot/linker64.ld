/* Copy from bbl-ucore : https://ring00.github.io/bbl-ucore      */

/* Simple linker script for the ucore kernel.
   See the GNU ld 'info' manual ("info ld") to learn the syntax. */

OUTPUT_ARCH(riscv)
ENTRY(_start)

BASE_ADDRESS  = 0xffffffff80200000;

SECTIONS
{
    /* Load the kernel at this address: "." means the current address */
    . = BASE_ADDRESS;
    start = .;

    .text : {
        stext = .;
        *(.text.entry)
        strampoline = .;
        *(.text.trampoline)
        _copy_user_start = .;
        *(.text.copy_user)
        _copy_user_end = .;
        *(.text .text.*)
        . = ALIGN(4K);
        etext = .;
    }

    .rodata : {
        srodata = .;
        PROVIDE( __global_pointer$ = . + 0x800 );
        *(.rodata .rodata.*)
        . = ALIGN(4K);
        erodata = .;
    }
    .data : {
        sdata = .;
        *(.data .data.*)
        *(.sdata .sdata.*)
        /* . = ALIGN(4K); */
        edata = .;
    }

    .stack : {
        sstack = .;
        *(.bss.stack)
        /* . = ALIGN(4K); */
        estack = .;
    }

    .bss : {
        sbss = .;
        *(.bss .bss.*)
        *(.sbss .sbss.*)
        . = ALIGN(4K);
        ebss = .;
    }

    PROVIDE(end = .);
}
