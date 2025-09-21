use anyhow::Result;

pub const MAX_CAPACITY: usize = 1000;

pub struct Queue<T> {
    capacity: usize,
    head: usize,
    tail: usize,
    buffer: [Option<T>; MAX_CAPACITY],
}

impl<T> Queue<T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, MAX_CAPACITY),
            head: 0,
            tail: 0,
            buffer: std::array::from_fn(|_| None),
        }
    }

    fn get_first_position(&self) -> usize {
        self.head
    }

    fn get_next_first_position(&self) -> usize {
        let curr = self.get_first_position();
        let mut pos = curr + 1;

        // handle the wrap around
        if pos >= self.capacity {
            pos = 0;
        }

        pos
    }

    fn get_curr_position(&self) -> usize {
        self.tail
    }

    fn get_next_position(&self) -> usize {
        let curr = self.get_curr_position();

        // at the start, current position is empty, so, we shouldn't
        // advance position
        if self.buffer[curr].is_none() {
            return curr;
        }

        let mut pos = curr + 1;

        // handle the wrap around
        if pos >= self.capacity {
            pos = 0;
        }

        pos
    }

    fn has_wrapped(&self) -> bool {
        self.buffer[self.get_next_position()].is_some()
    }

    pub fn is_empty(&self) -> bool {
        // a queue without capacity is always empty
        if self.capacity == 0 {
            return true;
        }

        // means that there is no length and the next doesnt exist so... empty
        if self.head == self.tail && self.buffer[self.head].is_none() {
            return true;
        }

        false
    }

    pub fn push(&mut self, item: T) {
        if self.capacity == 0 {
            return;
        }

        // tail should always be the last item
        self.tail = self.get_next_position();
        self.buffer[self.tail] = Some(item);

        // at this point, the tail has wrapped around and we want
        // the head to follow the tail
        if self.has_wrapped() {
            self.head = self.get_next_position();
        }
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.is_empty() {
            return None;
        }

        let first_pos = self.get_first_position();
        let item = self.buffer[first_pos].take();

        // change the head / tail positions
        self.head = self.get_next_first_position();

        // if the buffer is empty, leave it as is, do nothing
        if self.is_empty() || item.is_none() || self.buffer[self.head].is_none() {
            self.head = self.tail;
        }

        item
    }

    pub fn peek(&self) -> Option<&T> {
        self.buffer[self.get_first_position()].as_ref()
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.buffer = std::array::from_fn(|_| None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() -> Result<()> {
        let test_values = [
            // (capacity_input, capacity_expected)
            (0, 1),
            (1, 1),
            (10, 10),
            (MAX_CAPACITY, MAX_CAPACITY),
            (MAX_CAPACITY + 1, MAX_CAPACITY),
        ];

        for spec in test_values {
            let queue: Queue<i32> = Queue::new(spec.0);
            assert_eq!(queue.head, 0);
            assert_eq!(queue.tail, 0);
            assert_eq!(queue.capacity, spec.1);
        }

        Ok(())
    }

    #[test]
    fn test_is_empty() -> Result<()> {
        let mut queue: Queue<i32> = Queue::new(5);
        assert_eq!(queue.head, 0);

        assert!(queue.is_empty());
        queue.push(1);
        assert!(!queue.is_empty());
        let _ = queue.pop();
        assert!(queue.is_empty());

        queue.head = 3;
        queue.tail = 3;
        assert!(queue.is_empty());

        Ok(())
    }

    #[test]
    fn test_push() -> Result<()> {
        let mut queue: Queue<i32> = Queue::new(5);
        assert_eq!(queue.head, 0);

        let test_values = [
            // (value, after_head, after_tail)
            (1, 0, 0),  // first in queue
            (10, 0, 1), // middle of queue, a
            (15, 0, 2), // middle of queue, b
            (20, 0, 3), // middle of queue, c
            (25, 0, 4), // last of queue
            (30, 1, 0), // tail wrap
            (35, 2, 1), // after wrap
            (40, 3, 2), // loop around...
            (45, 4, 3), // loop around...
            (50, 0, 4), // loop around...
            (55, 1, 0), // after second wrap
        ];

        for spec in test_values {
            queue.push(spec.0);

            let curr = queue.get_curr_position();
            assert_eq!(queue.buffer[curr], Some(spec.0));
            assert_eq!(queue.head, spec.1);
            assert_eq!(queue.tail, spec.2);
        }

        Ok(())
    }

    #[test]
    fn test_pop_no_wrap() -> Result<()> {
        let mut queue: Queue<i32> = Queue::new(5);
        assert_eq!(queue.head, 0);

        // prepate the test with pushes
        let values = [1, 10, 15, 20];
        for val in values {
            queue.push(val);
        }
        assert_eq!(queue.head, 0);
        assert_eq!(queue.tail, values.len() - 1);

        let test_values = [
            // (value, after_head, after_tail)
            (Some(values[0]), 1, 3), // first in queue
            (Some(values[1]), 2, 3), // middle of queue, a
            (Some(values[2]), 3, 3), // middle of queue, b
            (Some(values[3]), 3, 3), // middle of queue, c
            (None, 3, 3),            // after final
        ];
        for spec in test_values {
            let prev = queue.get_first_position();

            let res = queue.pop();
            assert_eq!(queue.buffer[prev], None);
            assert_eq!(res, spec.0);
            assert_eq!(queue.head, spec.1);
            assert_eq!(queue.tail, spec.2);
        }

        Ok(())
    }

    #[test]
    fn test_pop_wrap() -> Result<()> {
        let mut queue: Queue<i32> = Queue::new(5);
        assert_eq!(queue.head, 0);

        // prepate the test with pushes
        let values = [1, 10, 15, 20, 25, 30];
        for val in values {
            queue.push(val);
        }
        assert_eq!(queue.head, 1);
        assert_eq!(queue.tail, 0);

        let test_values = [
            // (value, after_head, after_tail)
            (Some(values[1]), 2, 0), // first in queue
            (Some(values[2]), 3, 0), // middle of queue, a
            (Some(values[3]), 4, 0), // middle of queue, b
            (Some(values[4]), 0, 0), // middle of queue, c
            (Some(values[5]), 0, 0), // final
            (None, 0, 0),            // after final
        ];
        for spec in test_values {
            let prev = queue.get_first_position();

            let res = queue.pop();
            assert_eq!(queue.buffer[prev], None);
            assert_eq!(res, spec.0);
            assert_eq!(queue.head, spec.1);
            assert_eq!(queue.tail, spec.2);
        }

        Ok(())
    }

    #[test]
    fn test_peek() -> Result<()> {
        let mut queue: Queue<i32> = Queue::new(5);
        assert_eq!(queue.head, 0);

        // prepate the test with pushes
        let values = [1, 10, 15, 20, 25, 30];
        for val in values {
            queue.push(val);
        }
        assert_eq!(queue.head, 1);
        assert_eq!(queue.tail, 0);

        let res = *queue.peek().unwrap();
        assert_eq!(res, values[1]);

        let res = *queue.peek().unwrap();
        assert_eq!(res, values[1]);

        Ok(())
    }

    #[test]
    fn test_clear() -> Result<()> {
        let mut queue: Queue<i32> = Queue::new(5);
        assert_eq!(queue.head, 0);

        // prepate the test with pushes
        let values = [1, 10, 15];
        for val in values {
            queue.push(val);
        }
        assert_eq!(queue.head, 0);
        assert_eq!(queue.tail, 2);

        queue.clear();
        assert_eq!(queue.head, 0);
        assert_eq!(queue.tail, 0);

        queue.clear();
        assert_eq!(queue.head, 0);
        assert_eq!(queue.tail, 0);

        Ok(())
    }

    #[test]
    fn test_integration() -> Result<()> {
        let mut queue: Queue<i32> = Queue::new(5);
        assert_eq!(queue.head, 0);

        queue.push(1);
        assert_eq!(queue.head, 0);
        assert_eq!(queue.tail, 0);

        let res = queue.pop().unwrap();
        assert_eq!(res, 1);
        assert_eq!(queue.head, 0);
        assert_eq!(queue.tail, 0);

        queue.push(10);
        queue.push(20);
        queue.push(30);
        assert_eq!(queue.head, 0);
        assert_eq!(queue.tail, 2);

        let res = queue.pop().unwrap();
        assert_eq!(res, 10);
        assert_eq!(queue.head, 1);
        assert_eq!(queue.tail, 2);

        let res = *queue.peek().unwrap();
        assert_eq!(res, 20);
        assert_eq!(queue.head, 1);
        assert_eq!(queue.tail, 2);

        queue.push(40);
        assert_eq!(queue.head, 1);
        assert_eq!(queue.tail, 3);

        queue.clear();
        assert_eq!(queue.head, 0);
        assert_eq!(queue.tail, 0);

        Ok(())
    }
}
