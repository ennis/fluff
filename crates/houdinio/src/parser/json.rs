//! JSON geometry format
use crate::error::Error;
use crate::parser::Event;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ParserState {
    Array,
    Map,
}

pub(crate) struct ParserImpl<'a> {
    data: &'a str,
    state: Vec<ParserState>,
    depth: usize,
}

impl<'a> ParserImpl<'a> {
    pub(crate) fn new(data: &'a str) -> Self {
        Self {
            data,
            state: Vec::new(),
            depth: 0,
        }
    }

    fn skip_ws(&mut self) {
        self.data = self.data.trim_start_matches(|c: char| c.is_ascii_whitespace());
    }

    pub(crate) fn next(&mut self) -> Option<Event> {
        self.skip_ws();
        let n = match self.data.chars().next() {
            Some('[') => {
                self.data = &self.data[1..];
                self.state.push(ParserState::Array);
                Some(Event::BeginArray)
            }
            Some('{') => {
                self.data = &self.data[1..];
                self.state.push(ParserState::Map);
                Some(Event::BeginMap)
            }
            Some(']') => {
                self.state.pop()?;
                self.data = &self.data[1..];
                Some(Event::EndArray)
            }
            Some('}') => {
                self.state.pop()?;
                self.data = &self.data[1..];
                Some(Event::EndMap)
            }
            Some(',') => {
                // TODO maybe do some basic syntax checking here,
                // but in general we assume that the input is well-formed.
                self.data = &self.data[1..];
                return self.next();
            }
            Some(':') => {
                // TODO same as above
                self.data = &self.data[1..];
                return self.next();
            }
            Some(_) => {
                let mut des = serde_json::Deserializer::from_str(self.data).into_iter();
                let event = match des.next() {
                    Some(Ok(serde_json::Value::String(value))) => Event::String(value),
                    Some(Ok(serde_json::Value::Number(value))) => {
                        Event::Float(value.as_f64().expect("invalid number in json repr"))
                    }
                    Some(Ok(serde_json::Value::Bool(value))) => Event::Boolean(value),
                    Some(Ok(serde_json::Value::Null)) => {
                        panic!("null");
                    }
                    _ => {
                        panic!("unexpected json value");
                    }
                };
                self.data = &self.data[des.byte_offset()..];
                Some(event)
            }
            None => None,
        };
        //eprintln!("next: {:?}", n);
        n
    }

    pub(crate) fn skip(&mut self) {
        let mut depth = 0;
        while let Some(e) = self.next() {
            match e {
                Event::BeginArray | Event::BeginMap => {
                    depth += 1;
                    //eprintln!("skip: begin array/map {depth}");
                }
                Event::EndArray | Event::EndMap => {
                    //eprintln!("skip: end array/map {depth}");
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {
                    if depth == 0 {
                        return;
                    }
                }
            }
        }
    }

    /*/// Skips the next value.
    ///
    /// Returns `None` if the end of the input was reached, `()` otherwise.
    pub(crate) fn skip2(&mut self) -> Option<()> {
        //eprintln!("skip");
        match self.next() {
            Some(Event::BeginArray) => {
                while self.next() != Some(Event::EndArray) {
                    self.skip()?;
                }
                Some(())
            }
            Some(Event::BeginMap) => {
                while self.next() != Some(Event::EndMap) {
                    self.skip()?;
                }
                Some(())
            }
            Some(_) => Some(()),
            None => None,
        }
    }*/

    /// Reads a string from the input.
    pub(crate) fn str(&mut self) -> Result<String, Error> {
        match self.next().ok_or(Error::EarlyEof)? {
            Event::String(s) => Ok(s),
            _ => Err(Error::Malformed),
        }
    }

    /// Expects the beginning of an array.
    pub(crate) fn begin_array(&mut self) -> Result<(), Error> {
        match self.next().ok_or(Error::EarlyEof)? {
            Event::BeginArray => Ok(()),
            _ => Err(Error::Malformed),
        }
    }

    /// Expects the end of an array.
    pub(crate) fn end_array(&mut self) -> Result<(), Error> {
        match self.next().ok_or(Error::EarlyEof)? {
            Event::EndArray => Ok(()),
            _ => Err(Error::Malformed),
        }
    }

    fn begin_map(&mut self) -> Result<(), Error> {
        match self.next().ok_or(Error::EarlyEof)? {
            Event::BeginMap => Ok(()),
            _ => Err(Error::Malformed),
        }
    }

    fn end_map(&mut self) -> Result<(), Error> {
        match self.next().ok_or(Error::EarlyEof)? {
            Event::EndMap => Ok(()),
            _ => Err(Error::Malformed),
        }
    }

    pub(crate) fn eof(&mut self) -> bool {
        self.skip_ws();
        match self.data.chars().next() {
            Some(']' | '}') if self.state.is_empty() => true,
            None => true,
            _ => false,
        }
    }

    pub(crate) fn integer(&mut self) -> Result<i64, Error> {
        match self.next().ok_or(Error::EarlyEof)? {
            Event::Float(f) => Ok(f as i64),
            Event::Integer(i) => Ok(i),
            _ => Err(Error::Malformed),
        }
    }

    pub(crate) fn boolean(&mut self) -> Result<bool, Error> {
        match self.next().ok_or(Error::EarlyEof)? {
            Event::Boolean(b) => Ok(b),
            _ => Err(Error::Malformed),
        }
    }

    pub(crate) fn read_int32_array(&mut self) -> Result<Vec<i32>, Error> {
        let mut v = Vec::new();
        self.read_array(|p| {
            while let Some(e) = p.next() {
                v.push(e.as_integer().ok_or(Error::Malformed)? as i32);
            }
            Ok(())
        })?;
        Ok(v)
    }

    pub(crate) fn read_fp32_array(&mut self) -> Result<Vec<f32>, Error> {
        let mut v = Vec::new();
        self.read_array(|p| {
            while let Some(e) = p.next() {
                v.push(e.as_float().ok_or(Error::Malformed)? as f32);
            }
            Ok(())
        })?;
        Ok(v)
    }

    pub(crate) fn read_array<F>(&mut self, mut f: F) -> Result<(), Error>
    where
        F: FnMut(&mut Self) -> Result<(), Error>,
    {
        //eprintln!("{}array", "  ".repeat(self.depth));
        self.begin_array()?;
        let mut subparser = ParserImpl {
            data: self.data,
            state: Vec::new(),
            depth: self.depth + 1,
        };
        f(&mut subparser)?;
        self.data = subparser.data;
        self.end_array()?;
        Ok(())
    }

    pub(crate) fn read_kvarray<F>(&mut self, mut f: F) -> Result<(), Error>
    where
        F: FnMut(&mut Self, &str) -> Result<(), Error>,
    {
        //eprintln!("{}kvarray", "  ".repeat(self.depth));
        self.begin_array()?;
        let mut subparser = ParserImpl {
            data: self.data,
            state: Vec::new(),
            depth: self.depth + 1,
        };
        while let Some(e) = subparser.next() {
            let key = e.as_str().ok_or(Error::Malformed)?;
            //eprintln!("{}key: {}", "  ".repeat(subparser.depth), key);
            f(&mut subparser, key)?;
        }
        self.data = subparser.data;
        self.end_array()?;
        Ok(())
    }

    pub(crate) fn read_map<F>(&mut self, mut f: F) -> Result<(), Error>
    where
        F: FnMut(&mut Self, &str) -> Result<(), Error>,
    {
        //eprintln!("{}map", "  ".repeat(self.depth));
        self.begin_map()?;
        let mut subparser = ParserImpl {
            data: self.data,
            state: Vec::new(),
            depth: self.depth + 1,
        };
        while let Some(e) = subparser.next() {
            let key = e.as_str().ok_or(Error::Malformed)?;
            //eprintln!("{}key: {}", "  ".repeat(subparser.depth), key);
            f(&mut subparser, key)?;
        }
        self.data = subparser.data;
        self.end_map()?;
        Ok(())
    }
}
