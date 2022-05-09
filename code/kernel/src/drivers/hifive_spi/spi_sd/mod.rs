// From rcore-os/rCore-Tutorial-v3
// See https://github.com/rcore-os/rCore-Tutorial-v3/blob/main/LICENSE
// Distributed under GPLv3

#![allow(non_snake_case)]
// pub mod abstraction;
// pub mod platform;

pub mod abstraction;
pub mod platform;

use crate::sync::SleepMutex;

use super::BlockDevice;
use abstraction::*;
use alloc::boxed::Box;
use core::convert::TryInto;
use ftl_util::device::AsyncRet;

pub struct SDCard<T: SPIActions> {
    spi: T,
    spi_cs: u32,
    is_hc: bool,
}

/*
 * Start Data tokens:
 *         Tokens (necessary because at nop/idle (and CS active) only 0xff is
 *         on the data/command line)
 */
/** Data token start byte, Start Single Block Read */
pub const SD_START_DATA_SINGLE_BLOCK_READ: u8 = 0xFE;
#[allow(unused)]
/** Data token start byte, Start Multiple Block Read */
pub const SD_START_DATA_MULTIPLE_BLOCK_READ: u8 = 0xFE;
/** Data token start byte, Start Single Block Write */
pub const SD_START_DATA_SINGLE_BLOCK_WRITE: u8 = 0xFE;
/** Data token start byte, Start Multiple Block Write */
pub const SD_START_DATA_MULTIPLE_BLOCK_WRITE: u8 = 0xFC;

pub const SD_STOP_DATA_MULTIPLE_BLOCK_WRITE: u8 = 0xFD;

pub const SEC_LEN: usize = 512;

/** SD commands */
#[repr(u8)]
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
#[allow(unused)]
pub enum CMD {
    /** Software reset */
    CMD0 = 0,
    /** Check voltage range (SDC V2) */
    CMD8 = 8,
    /** Read CSD register */
    CMD9 = 9,
    /** Read CID register */
    CMD10 = 10,
    /** Stop to read data */
    CMD12 = 12,
    /** Change R/W block size */
    CMD16 = 16,
    /** Read block */
    CMD17 = 17,
    /** Read multiple blocks */
    CMD18 = 18,
    /** Number of blocks to erase (SDC) */
    ACMD23 = 23,
    /** Write a block */
    CMD24 = 24,
    /** Write multiple blocks */
    CMD25 = 25,
    /** Initiate initialization process (SDC) */
    ACMD41 = 41,
    /** Leading command for ACMD* */
    CMD55 = 55,
    /** Read OCR */
    CMD58 = 58,
    /** Enable/disable CRC check */
    CMD59 = 59,
    /** Custom CMD - just for testing */
    CMD1 = 0x1,
}

#[allow(unused)]
#[derive(Debug, Copy, Clone)]
pub enum InitError {
    CMDFailed(CMD, u8),
    CardCapacityStatusNotSet([u8; 4]),
    CannotGetCardInfo,
}

/**
 * Card Specific Data: CSD Register
 */
#[derive(Debug, Copy, Clone)]
pub struct SDCardCSD {
    pub CSDStruct: u8,        /* CSD structure */
    pub SysSpecVersion: u8,   /* System specification version */
    pub Reserved1: u8,        /* Reserved */
    pub TAAC: u8,             /* Data read access-time 1 */
    pub NSAC: u8,             /* Data read access-time 2 in CLK cycles */
    pub MaxBusClkFrec: u8,    /* Max. bus clock frequency */
    pub CardComdClasses: u16, /* Card command classes */
    pub RdBlockLen: u8,       /* Max. read data block length */
    pub PartBlockRead: u8,    /* Partial blocks for read allowed */
    pub WrBlockMisalign: u8,  /* Write block misalignment */
    pub RdBlockMisalign: u8,  /* Read block misalignment */
    pub DSRImpl: u8,          /* DSR implemented */
    pub Reserved2: u8,        /* Reserved */
    pub DeviceSize: u32,      /* Device Size */
    //MaxRdCurrentVDDMin: u8,   /* Max. read current @ VDD min */
    //MaxRdCurrentVDDMax: u8,   /* Max. read current @ VDD max */
    //MaxWrCurrentVDDMin: u8,   /* Max. write current @ VDD min */
    //MaxWrCurrentVDDMax: u8,   /* Max. write current @ VDD max */
    //DeviceSizeMul: u8,        /* Device size multiplier */
    pub EraseGrSize: u8,         /* Erase group size */
    pub EraseGrMul: u8,          /* Erase group size multiplier */
    pub WrProtectGrSize: u8,     /* Write protect group size */
    pub WrProtectGrEnable: u8,   /* Write protect group enable */
    pub ManDeflECC: u8,          /* Manufacturer default ECC */
    pub WrSpeedFact: u8,         /* Write speed factor */
    pub MaxWrBlockLen: u8,       /* Max. write data block length */
    pub WriteBlockPaPartial: u8, /* Partial blocks for write allowed */
    pub Reserved3: u8,           /* Reserded */
    pub ContentProtectAppli: u8, /* Content protection application */
    pub FileFormatGroup: u8,     /* File format group */
    pub CopyFlag: u8,            /* Copy flag (OTP) */
    pub PermWrProtect: u8,       /* Permanent write protection */
    pub TempWrProtect: u8,       /* Temporary write protection */
    pub FileFormat: u8,          /* File Format */
    pub ECC: u8,                 /* ECC code */
    pub CSD_CRC: u8,             /* CSD CRC */
    pub Reserved4: u8,           /* always 1*/
}

/**
 * Card Identification Data: CID Register
 */
#[derive(Debug, Copy, Clone)]
pub struct SDCardCID {
    pub ManufacturerID: u8, /* ManufacturerID */
    pub OEM_AppliID: u16,   /* OEM/Application ID */
    pub ProdName1: u32,     /* Product Name part1 */
    pub ProdName2: u8,      /* Product Name part2*/
    pub ProdRev: u8,        /* Product Revision */
    pub ProdSN: u32,        /* Product Serial Number */
    pub Reserved1: u8,      /* Reserved1 */
    pub ManufactDate: u16,  /* Manufacturing Date */
    pub CID_CRC: u8,        /* CID CRC */
    pub Reserved2: u8,      /* always 1 */
}

/**
 * Card information
 */
#[derive(Debug, Copy, Clone)]
pub struct SDCardInfo {
    pub SD_csd: SDCardCSD,
    pub SD_cid: SDCardCID,
    pub CardCapacity: u64,  /* Card Capacity */
    pub CardBlockSize: u64, /* Card Block Size */
    pub is_hc: bool,        /* Card is SDHC */
}

impl<T: SPIActions> SDCard<T> {
    pub fn new(spi: T, spi_cs: u32) -> Self {
        Self {
            spi,
            spi_cs,
            is_hc: false,
        }
    }

    fn HIGH_SPEED_ENABLE(&mut self) {
        self.spi.set_clk_rate(10000000);
    }

    fn CS_HIGH(&mut self) {
        self.spi.switch_cs(false, 0);
    }

    fn CS_LOW(&mut self) {
        self.spi.switch_cs(true, 0);
    }

    fn lowlevel_init(&mut self) {
        // gpiohs::set_direction(self.cs_gpionum, gpio::direction::OUTPUT);
        // at first clock rate shall be low (below 200khz)
        println!("lowlevel_init start");
        self.spi.init();
        println!("lowlevel_init 1");
        self.spi.set_clk_rate(150000);
        println!("lowlevel_init end");
    }

    /*
     * Initializes the SD/SD communication in SPI mode.
     * @param  None
     * @retval The SD Response info if succeeeded, otherwise Err
     */
    pub fn init(&mut self) -> Result<SDCardInfo, InitError> {
        stack_trace!();
        println!("SDCard init start");
        /* Initialize SD_SPI */
        self.lowlevel_init();
        /* An empty frame for commands */
        let mut frame = [0u8; 4];
        /* is hc card */
        let mut is_hc = false;
        /* NOTE: this reset doesn't always seem to work if the SD access was broken off in the
         * middle of an operation: CMDFailed(CMD0, 127). */

        /* Send dummy byte 0xFF, 10 times with CS high */
        /* Rise CS and MOSI for 80 clocks cycles */
        /* Send dummy byte 0xFF */
        println!("SDCard init 0");
        self.spi.switch_cs(false, 0);
        println!("SDCard init 1");
        self.spi.configure(
            2,    // use lines
            8,    // bits per word
            true, // endian: big-endian
        );
        println!("SDCard init 2");
        self.write_data(&[0xff; 10]);
        println!("SDCard init 3");
        /*------------Put SD in SPI mode--------------*/
        /* SD initialized and set to SPI mode properly */

        /* Send software reset */
        let mut result = 0;
        let mut retry_times = 0;
        if let Err(()) = self.retry_cmd(CMD::CMD0, 0, 0x95, 0x01, 2000) {
            println!("SDCard init error in retry_cmd");
            return Err(InitError::CMDFailed(CMD::CMD0, 0));
        }

        println!("SDCard init 4");

        /* Check voltage range */
        self.send_cmd(CMD::CMD8, 0x01AA, 0x87);
        result = self.get_response(); // 0x01 or 0x05
        self.read_data(&mut frame);
        self.end_cmd();
        self.end_cmd();

        if result != 0x01 {
            // Standard Capacity Card
            result = 0xff;

            while result != 0x00 {
                self.send_cmd(CMD::CMD55, 0, 0);
                self.get_response();
                self.end_cmd();
                self.send_cmd(CMD::ACMD41, 0x40000000, 0);
                result = self.get_response();
                self.end_cmd();
            }
        } else {
            // Need further discrimination
            let mut cnt = 0;

            cnt = 0xff;
            while result != 0x00 {
                self.send_cmd(CMD::CMD55, 0, 0);
                self.get_response();
                self.end_cmd();
                self.send_cmd(CMD::ACMD41, 0x40000000, 0);
                result = self.get_response();
                self.end_cmd();

                if cnt == 0 {
                    return Err(InitError::CMDFailed(CMD::ACMD41, 0));
                } else {
                    cnt -= 1;
                }
            }

            cnt = 0xff;
            result = 0xff;
            while (result != 0x0) && (result != 0x1) {
                self.send_cmd(CMD::CMD58, 0, 1);
                result = self.get_response();
                self.read_data(&mut frame);
                self.end_cmd();

                if cnt == 0 {
                    return Err(InitError::CMDFailed(CMD::CMD58, 0));
                } else {
                    cnt -= 1;
                }
            }

            for i in 0..4 {
                println!("OCR[{}]: 0x{:x}", i, frame[i]);
            }

            if (frame[0] & 0x40) != 0 {
                self.is_hc = true;
            }
        }

        println!("SDCard init 5");

        self.spi.switch_cs(false, 0);
        self.write_data(&[0xff; 10]);

        self.HIGH_SPEED_ENABLE();
        match self.get_cardinfo() {
            Ok(info) => Ok(info),
            Err(_) => Err(InitError::CannotGetCardInfo),
        }
    }

    fn write_data(&mut self, data: &[u8]) {
        self.spi.configure(
            2,    // use lines
            8,    // bits per word
            true, // endian: big-endian
        );
        self.spi.send_data(self.spi_cs, data);
    }

    fn read_data(&mut self, data: &mut [u8]) {
        self.spi.configure(
            2,    // use lines
            8,    // bits per word
            true, // endian: big-endian
        );
        self.spi.recv_data(self.spi_cs, data);
    }

    ///Send 5 bytes command to the SD card.
    ///
    /// cmd: The user expected command to send to SD card.
    ///
    /// arg: The command argument.
    ///
    /// crc: The CRC.
    ///
    fn send_cmd(&mut self, cmd: CMD, arg: u32, crc: u8) {
        self.CS_LOW();
        /* Send the Cmd bytes */
        self.write_data(&[
            /* Construct byte 1 */
            ((cmd as u8) | 0x40),
            /* Construct byte 2 */
            (arg >> 24) as u8,
            /* Construct byte 3 */
            ((arg >> 16) & 0xff) as u8,
            /* Construct byte 4 */
            ((arg >> 8) & 0xff) as u8,
            /* Construct byte 5 */
            (arg & 0xff) as u8,
            /* CRC */
            crc,
        ]);
    }

    /* Send end-command sequence to SD card */
    fn end_cmd(&mut self) {
        // TODO Does end operation need to deassert CS?
        self.CS_HIGH();
        /* Send the cmd byte */
        self.write_data(&[0xff; 4]);
    }

    /*
     * Returns the SD response.
     * @param  None
     * @retval The SD Response:
     *         - 0xFF: Sequence failed
     *         - 0: Sequence succeed
     */
    fn get_response(&mut self) -> u8 {
        let result = &mut [0xffu8];
        let mut timeout = 0x0FFF;
        /* Check if response is got or a timeout is happen */
        while timeout != 0 {
            self.read_data(result);
            /* Right response got */
            if result[0] != 0xFF {
                return result[0];
            }
            timeout -= 1;
        }
        /* After time out */
        return 0xFF;
    }

    /*
     * Get SD card data response.
     * @param  None
     * @retval The SD status: Read data response xxx0<status>1
     *         - status 010: Data accecpted
     *         - status 101: Data rejected due to a crc error
     *         - status 110: Data rejected due to a Write error.
     *         - status 111: Data rejected due to other error.
     */
    fn get_dataresponse(&mut self) -> u8 {
        let response = &mut [0u8];
        /* Read resonse */
        self.read_data(response);
        /* Mask unused bits */
        response[0] &= 0x1F;
        if response[0] != 0x05 {
            return 0xFF;
        }
        /* Wait null data */
        self.read_data(response);
        while response[0] == 0 {
            self.read_data(response);
        }
        /* Return response */
        return 0;
    }

    /*
     * Read the CSD card register
     *         Reading the contents of the CSD register in SPI mode is a simple
     *         read-block transaction.
     * @param  SD_csd: pointer on an SCD register structure
     * @retval The SD Response:
     *         - `Err()`: Sequence failed
     *         - `Ok(info)`: Sequence succeed
     */
    fn get_csdregister(&mut self) -> Result<SDCardCSD, ()> {
        let mut csd_tab = [0u8; 18];
        /* Send CMD9 (CSD register) */
        self.send_cmd(CMD::CMD9, 0, 0);
        /* Wait for response in the R1 format (0x00 is no errors) */
        if self.get_response() != 0x00 {
            self.end_cmd();
            return Err(());
        }
        if self.get_response() != SD_START_DATA_SINGLE_BLOCK_READ {
            self.end_cmd();
            return Err(());
        }
        /* Store CSD register value on csd_tab */
        /* Get CRC bytes (not really needed by us, but required by SD) */
        self.read_data(&mut csd_tab);
        self.end_cmd();
        self.end_cmd();
        /* see also: https://cdn-shop.adafruit.com/datasheets/TS16GUSDHC6.pdf */
        return Ok(SDCardCSD {
            /* Byte 0 */
            CSDStruct: (csd_tab[0] & 0xC0) >> 6,
            SysSpecVersion: (csd_tab[0] & 0x3C) >> 2,
            Reserved1: csd_tab[0] & 0x03,
            /* Byte 1 */
            TAAC: csd_tab[1],
            /* Byte 2 */
            NSAC: csd_tab[2],
            /* Byte 3 */
            MaxBusClkFrec: csd_tab[3],
            /* Byte 4, 5 */
            CardComdClasses: (u16::from(csd_tab[4]) << 4) | ((u16::from(csd_tab[5]) & 0xF0) >> 4),
            /* Byte 5 */
            RdBlockLen: csd_tab[5] & 0x0F,
            /* Byte 6 */
            PartBlockRead: (csd_tab[6] & 0x80) >> 7,
            WrBlockMisalign: (csd_tab[6] & 0x40) >> 6,
            RdBlockMisalign: (csd_tab[6] & 0x20) >> 5,
            DSRImpl: (csd_tab[6] & 0x10) >> 4,
            Reserved2: 0,
            // DeviceSize: (csd_tab[6] & 0x03) << 10,
            /* Byte 7, 8, 9 */
            DeviceSize: ((u32::from(csd_tab[7]) & 0x3F) << 16)
                | (u32::from(csd_tab[8]) << 8)
                | u32::from(csd_tab[9]),
            /* Byte 10 */
            EraseGrSize: (csd_tab[10] & 0x40) >> 6,
            /* Byte 10, 11 */
            EraseGrMul: ((csd_tab[10] & 0x3F) << 1) | ((csd_tab[11] & 0x80) >> 7),
            /* Byte 11 */
            WrProtectGrSize: (csd_tab[11] & 0x7F),
            /* Byte 12 */
            WrProtectGrEnable: (csd_tab[12] & 0x80) >> 7,
            ManDeflECC: (csd_tab[12] & 0x60) >> 5,
            WrSpeedFact: (csd_tab[12] & 0x1C) >> 2,
            /* Byte 12,13 */
            MaxWrBlockLen: ((csd_tab[12] & 0x03) << 2) | ((csd_tab[13] & 0xC0) >> 6),
            /* Byte 13 */
            WriteBlockPaPartial: (csd_tab[13] & 0x20) >> 5,
            Reserved3: 0,
            ContentProtectAppli: (csd_tab[13] & 0x01),
            /* Byte 14 */
            FileFormatGroup: (csd_tab[14] & 0x80) >> 7,
            CopyFlag: (csd_tab[14] & 0x40) >> 6,
            PermWrProtect: (csd_tab[14] & 0x20) >> 5,
            TempWrProtect: (csd_tab[14] & 0x10) >> 4,
            FileFormat: (csd_tab[14] & 0x0C) >> 2,
            ECC: (csd_tab[14] & 0x03),
            /* Byte 15 */
            CSD_CRC: (csd_tab[15] & 0xFE) >> 1,
            Reserved4: 1,
            /* Return the reponse */
        });
    }

    ///
    /// Read the CID card register.
    ///         Reading the contents of the CID register in SPI mode is a simple
    ///         read-block transaction.
    ///
    /// @param  SD_cid: pointer on an CID register structure
    ///
    /// @retval The SD Response:
    ///         - `Err()`: Sequence failed
    ///         - `Ok(info)`: Sequence succeed
    ///
    fn get_cidregister(&mut self) -> Result<SDCardCID, ()> {
        let mut cid_tab = [0u8; 18];
        /* Send CMD10 (CID register) */
        self.send_cmd(CMD::CMD10, 0, 0);
        /* Wait for response in the R1 format (0x00 is no errors) */
        if self.get_response() != 0x00 {
            self.end_cmd();
            return Err(());
        }
        if self.get_response() != SD_START_DATA_SINGLE_BLOCK_READ {
            self.end_cmd();
            return Err(());
        }
        /* Store CID register value on cid_tab */
        /* Get CRC bytes (not really needed by us, but required by SD) */
        self.read_data(&mut cid_tab);
        self.end_cmd();
        return Ok(SDCardCID {
            /* Byte 0 */
            ManufacturerID: cid_tab[0],
            /* Byte 1, 2 */
            OEM_AppliID: (u16::from(cid_tab[1]) << 8) | u16::from(cid_tab[2]),
            /* Byte 3, 4, 5, 6 */
            ProdName1: (u32::from(cid_tab[3]) << 24)
                | (u32::from(cid_tab[4]) << 16)
                | (u32::from(cid_tab[5]) << 8)
                | u32::from(cid_tab[6]),
            /* Byte 7 */
            ProdName2: cid_tab[7],
            /* Byte 8 */
            ProdRev: cid_tab[8],
            /* Byte 9, 10, 11, 12 */
            ProdSN: (u32::from(cid_tab[9]) << 24)
                | (u32::from(cid_tab[10]) << 16)
                | (u32::from(cid_tab[11]) << 8)
                | u32::from(cid_tab[12]),
            /* Byte 13, 14 */
            Reserved1: (cid_tab[13] & 0xF0) >> 4,
            ManufactDate: ((u16::from(cid_tab[13]) & 0x0F) << 8) | u16::from(cid_tab[14]),
            /* Byte 15 */
            CID_CRC: (cid_tab[15] & 0xFE) >> 1,
            Reserved2: 1,
        });
    }

    ///
    /// Returns information about specific card.
    ///
    /// cardinfo: pointer to a SD_CardInfo structure that contains all SD
    ///         card information.
    ///
    /// return: The SD Response:
    ///         - `Err(())`: Sequence failed
    ///         - `Ok(info)`: Sequence succeed
    ///
    fn get_cardinfo(&mut self) -> Result<SDCardInfo, ()> {
        let mut info = SDCardInfo {
            SD_csd: self.get_csdregister()?,
            SD_cid: self.get_cidregister()?,
            CardCapacity: 0,
            CardBlockSize: 0,
            is_hc: false,
        };
        info.CardBlockSize = 1 << u64::from(info.SD_csd.RdBlockLen);
        info.CardCapacity = (u64::from(info.SD_csd.DeviceSize) + 1) * 1024 * info.CardBlockSize;

        Ok(info)
    }

    fn retry_cmd(
        &mut self,
        cmd: CMD,
        arg: u32,
        crc: u8,
        expect: u8,
        retry_times: u32,
    ) -> Result<(), ()> {
        for i in 0..retry_times {
            self.send_cmd(cmd, arg, crc);
            let resp = self.get_response();
            self.end_cmd();
            if resp == expect {
                return Ok(());
            }
            if i % 10 == 0 {
                println!("retry_cmd: {} -> {}", i, resp);
            }
        }
        return Err(());
    }

    /*
     * Reads a block of data from te SD.
     * @param  data_buf: slice that receives the data read from the SD.
     * @param  sector: SD's internal address to read from.
     * @retval The SD Response:
     *         - `Err(())`: Sequence failed
     *         - `Ok(())`: Sequence succeed
     */
    pub fn read_sector(&mut self, data_buf: &mut [u8], sector: u32) -> Result<(), ()> {
        assert!(data_buf.len() >= SEC_LEN && (data_buf.len() % SEC_LEN) == 0);
        let sector: u32 = match self.is_hc {
            true => sector << 9,
            _ => sector,
        };

        let mut cur_sector = sector;

        for chunk in data_buf.chunks_mut(SEC_LEN) {
            /* Send CMD17 to read one block */
            self.send_cmd(CMD::CMD17, cur_sector, 0);
            /* Check if the SD acknowledged the read block command: R1 response (0x00: no errors) */
            let res = self.get_response();
            if res != 0x00 {
                self.end_cmd();
                return Err(());
            }

            //let mut dma_chunk = [0u32; SEC_LEN];
            let mut tmp_chunk = [0u8; SEC_LEN];
            let mut count = 0;
            let mut resp = self.get_response();
            if resp != SD_START_DATA_SINGLE_BLOCK_READ {
                resp = self.get_response();
                break;
            }

            /* Read the SD block data : read NumByteToRead data */
            self.read_data(&mut tmp_chunk);
            /* Place the data received as u32 units from DMA into the u8 target buffer */
            for (a, b) in chunk.iter_mut().zip(/*dma_chunk*/ tmp_chunk.iter()) {
                *a = *b;
            }
            /* Get CRC bytes (not really needed by us, but required by SD) */
            let mut frame = [0u8; 2];
            self.read_data(&mut frame);

            // put some dummy bytes
            self.end_cmd();
            self.end_cmd();

            cur_sector += SEC_LEN as u32;
        }

        // put some dummy bytes
        self.end_cmd();
        self.end_cmd();

        /* It is an error if not everything requested was read */
        Ok(())
    }

    /*
     * Writes a block to the SD
     * @param  data_buf: slice containing the data to be written to the SD.
     * @param  sector: address to write on.
     * @retval The SD Response:
     *         - `Err(())`: Sequence failed
     *         - `Ok(())`: Sequence succeed
     */
    pub fn write_sector(&mut self, data_buf: &[u8], sector: u32) -> Result<(), ()> {
        assert!(data_buf.len() >= SEC_LEN && (data_buf.len() % SEC_LEN) == 0);
        let sector: u32 = match self.is_hc {
            true => sector << 9,
            _ => sector,
        };

        let mut frame = [0xff, 0x00];
        if data_buf.len() == SEC_LEN {
            frame[1] = SD_START_DATA_SINGLE_BLOCK_WRITE;
            self.send_cmd(CMD::CMD24, sector, 0);
        } else {
            frame[1] = SD_START_DATA_MULTIPLE_BLOCK_WRITE;
            self.send_cmd(CMD::CMD55, 0, 0x01);
            self.get_response();
            self.end_cmd();
            self.send_cmd(
                CMD::ACMD23,
                (data_buf.len() / SEC_LEN).try_into().unwrap(),
                0,
            );
            self.get_response();
            self.end_cmd();
            self.send_cmd(CMD::CMD25, sector, 0);
        }
        /* Check if the SD acknowledged the write block command: R1 response (0x00: no errors) */
        if self.get_response() != 0x00 {
            //self.end_cmd();
            // return Err(());
        }
        //let mut dma_chunk = [0u32; SEC_LEN];
        let mut tmp_chunk = [0u8; SEC_LEN];
        for chunk in data_buf.chunks(SEC_LEN) {
            /* Send the data token to signify the start of the data */
            self.write_data(&frame);
            /* Write the block data to SD : write count data by block */
            for (a, &b) in /*dma_chunk*/ tmp_chunk.iter_mut().zip(chunk.iter()) {
                //*a = b.into();
                *a = b;
            }
            //self.write_data_dma(&mut dma_chunk);
            self.write_data(&mut tmp_chunk);
            /* Put dummy CRC bytes */
            self.write_data(&[0xff, 0xff]);
            /* Read data response */
            if self.get_dataresponse() != 0x05 {
                //self.end_cmd();
                //return Err(());
            }
        }

        if frame[1] == SD_START_DATA_MULTIPLE_BLOCK_WRITE {
            frame[1] = SD_STOP_DATA_MULTIPLE_BLOCK_WRITE;
            self.write_data(&frame);
        }

        self.end_cmd();
        self.end_cmd();

        /*
        self.send_cmd(CMD::CMD12, 0, 0);
        self.get_response();
        self.end_cmd();
        self.end_cmd();
        */

        Ok(())
    }
}

/** CS value passed to SPI controller, this is a dummy value as SPI0_CS3 is not mapping to anything
 * in the FPIOA */
const SD_CS: u32 = 0;

pub fn init_sdcard() -> SDCard<SPIImpl> {
    stack_trace!();
    // wait previous output
    // usleep(100000);

    println!("[FTL OS] init sdcard start");
    let spi = SPIImpl::new(abstraction::SPIDevice::QSPI2);
    let mut sd = SDCard::new(spi, SD_CS);
    let info = sd.init().unwrap();
    // assert!(num_sectors > 0);

    println!("[FTL OS] init sdcard end");
    sd
}

pub struct SDCardWrapper(SleepMutex<SDCard<SPIImpl>>);

impl SDCardWrapper {
    pub fn new() -> Self {
        Self(SleepMutex::new(init_sdcard()))
    }
}

impl BlockDevice for SDCardWrapper {
    fn sector_bpb(&self) -> usize {
        0
    }
    fn sector_bytes(&self) -> usize {
        512
    }
    fn read_block<'a>(&'a self, block_id: usize, buf: &'a mut [u8]) -> AsyncRet<'a> {
        Box::pin(async move {
            let lock = &mut *self.0.lock().await;
            println!("read block {}", block_id / 512);
            if let Err(_) = lock.read_sector(buf, block_id as u32) {
                panic!("read_block invalid {}", block_id);
            }
            Ok(())
        })
    }
    fn write_block<'a>(&'a self, block_id: usize, buf: &'a [u8]) -> AsyncRet<'a> {
        Box::pin(async move {
            let lock = &mut *self.0.lock().await;
            println!("write block {}", block_id);
            if let Err(_) = lock.write_sector(buf, block_id as u32) {
                panic!("write_block invalid {}", block_id / 512);
            }
            Ok(())
        })
    }
}
